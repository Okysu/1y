//! Coroutine-based async scheduler for the `1y` interpreter.
//!
//! Uses [`corosensei`] stackful coroutines. Each actor handler runs in its
//! own coroutine; `await` suspends and the scheduler runs other ready
//! coroutines.
//!
//! ## I/O multiplexing with mio
//!
//! The scheduler holds a [`mio::Poll`] for event-driven I/O. When
//! `socket.read_async` creates a Task, it registers the stream with mio
//! (via [`register_readable`]). The scheduler's `tick` calls
//! `mio::Poll::poll` to wait for I/O events, and only polls Tasks whose
//! streams are reported ready — avoiding O(n) polling of all parked Tasks.
//!
//! ## Architecture: thread-local yielder + thread-local scheduler
//!
//! The interpreter's `eval_expr` is a deeply recursive function. When it
//! hits `Expr::Await`, it needs to suspend the current coroutine. But the
//! `corosensei::Yielder` is only available inside the coroutine closure.
//!
//! Solution: store the yielder pointer in a `thread_local!`. When a
//! coroutine starts, it sets the thread-local yielder; when `eval_expr`
//! calls `await_task(task)`, it reads the thread-local yielder and
//! suspends.
//!
//! Similarly, `socket.read_async` needs to register streams with the
//! scheduler's mio::Poll, but it's a plain builtin function without
//! access to the scheduler. A second thread_local stores a pointer to
//! the active scheduler, set during `drain_mailboxes_async`.

use crate::interpreter::error::InterpreterError;
use crate::value::{TaskPoll, TaskRef, Value};
use corosensei::{Coroutine, CoroutineResult};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use mio::{Events, Interest, Poll, Token};

/// What a coroutine yields back to the scheduler.
pub enum YieldSignal {
    /// The coroutine is waiting on this task.
    AwaitTask(TaskRef),
}

/// What the scheduler sends when resuming a coroutine.
#[derive(Clone)]
pub enum ResumeSignal {
    /// The awaited task is ready; here is its value.
    TaskReady(Value),
}

/// Input to resume a coroutine: `None` for initial start, `Some(signal)` for resume after await.
pub type CoroInput = Option<ResumeSignal>;

// ---------------------------------------------------------------------------
// Thread-local yielder
// ---------------------------------------------------------------------------

/// Raw pointer to the current coroutine's Yielder, stored in thread-local.
type YielderPtr = *const corosensei::Yielder<CoroInput, YieldSignal>;

thread_local! {
    static CURRENT_YIELDER: RefCell<YielderPtr> = const { RefCell::new(core::ptr::null()) };
    static CURRENT_SCHEDULER: RefCell<*mut Scheduler> = const { RefCell::new(core::ptr::null_mut()) };
}

/// Suspend the current coroutine, waiting on `task`.
/// Returns the task's value when resumed.
///
/// # Panics
/// Panics if called outside a coroutine.
pub fn await_task(task: TaskRef) -> Value {
    CURRENT_YIELDER.with(|y| {
        let ptr = *y.borrow();
        if ptr.is_null() {
            panic!("await called outside of a coroutine");
        }
        // SAFETY: ptr was set by the coroutine currently running on this
        // thread. The Yielder is valid for the duration of the coroutine.
        let yielder: &corosensei::Yielder<CoroInput, YieldSignal> = unsafe { &*ptr };
        let result = yielder.suspend(YieldSignal::AwaitTask(task));
        // Re-set CURRENT_YIELDER after resume: while this coroutine was
        // suspended, the scheduler may have resumed another coroutine on
        // the same thread, overwriting CURRENT_YIELDER. `ptr` is still
        // valid (it's this coroutine's own yielder, captured before
        // suspend), so restore it for any subsequent await_task call.
        *y.borrow_mut() = ptr;
        match result {
            Some(ResumeSignal::TaskReady(v)) => v,
            None => Value::Nil,
        }
    })
}

/// Check if we're currently inside a coroutine.
pub fn in_coroutine() -> bool {
    CURRENT_YIELDER.with(|y| !y.borrow().is_null())
}

/// Set the thread-local scheduler pointer. Used by `drain_mailboxes_async`
/// when driving the scheduler tick-by-tick (instead of via
/// `run_until_complete`, which sets it internally). The caller MUST clear
/// it with `clear_current_scheduler` before returning, so that subsequent
/// top-level `await` calls don't dereference a stale pointer.
///
/// # Safety
/// `ptr` must point to a live `Scheduler` that outlives the next
/// `clear_current_scheduler` call. The caller must guarantee no other
/// `Scheduler` is concurrently active on this thread (single-threaded
/// runtime: trivially true).
pub unsafe fn set_current_scheduler(ptr: *mut Scheduler) {
    CURRENT_SCHEDULER.with(|s| {
        *s.borrow_mut() = ptr;
    });
}

/// Clear the thread-local scheduler pointer. Paired with
/// `set_current_scheduler`.
pub fn clear_current_scheduler() {
    CURRENT_SCHEDULER.with(|s| {
        *s.borrow_mut() = core::ptr::null_mut();
    });
}

// ---------------------------------------------------------------------------
// I/O registration (called by socket.read_async)
// ---------------------------------------------------------------------------

/// Register a TCP stream for readable events with the current scheduler's
/// mio::Poll. Called by `socket.read_async` when creating a Task.
///
/// Returns the allocated Token, or None if no scheduler is active (top-level
/// await without a running event loop).
pub fn register_readable(stream: &std::net::TcpStream, task: &TaskRef) -> Option<Token> {
    CURRENT_SCHEDULER.with(|s| {
        let ptr = *s.borrow();
        if ptr.is_null() {
            return None;
        }
        // SAFETY: ptr was set by drain_mailboxes_async, which holds a unique
        // borrow on the scheduler. Registration happens inside a coroutine
        // that runs within drain_mailboxes_async's scheduler.run_until_complete.
        let scheduler: &mut Scheduler = unsafe { &mut *ptr };
        scheduler.register_stream_readable(stream, task.clone())
    })
}

/// Register a TCP listener for readable events with the current scheduler's
/// mio::Poll. Called by `socket.accept_async` when creating a Task.
///
/// Returns the allocated Token, or None if no scheduler is active (top-level
/// await without a running event loop).
pub fn register_listener_readable(listener: &std::net::TcpListener, task: &TaskRef) -> Option<Token> {
    CURRENT_SCHEDULER.with(|s| {
        let ptr = *s.borrow();
        if ptr.is_null() {
            return None;
        }
        // SAFETY: ptr was set by drain_mailboxes_async, which holds a unique
        // borrow on the scheduler. Registration happens inside a coroutine
        // that runs within drain_mailboxes_async's scheduler.run_until_complete.
        let scheduler: &mut Scheduler = unsafe { &mut *ptr };
        scheduler.register_listener_readable(listener, task.clone())
    })
}

/// Deregister a TCP stream from the scheduler's mio::Poll.
/// Called when the Task completes or is consumed.
pub fn deregister_stream(token: Token) {
    CURRENT_SCHEDULER.with(|s| {
        let ptr = *s.borrow();
        if ptr.is_null() {
            return;
        }
        let scheduler: &mut Scheduler = unsafe { &mut *ptr };
        scheduler.deregister_stream(token);
    })
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

type Coro = Coroutine<CoroInput, YieldSignal, Result<Value, InterpreterError>>;

struct Parked {
    coroutine: Coro,
    task: TaskRef,
    /// If Some(token), this Task is waiting on an I/O event. Only poll when
    /// mio reports the token as ready. If None, it's a timer/combinator Task
    /// — poll every tick (legacy combinator path; timer Tasks with a
    /// deadline hint go into `timer_parked` instead).
    io_token: Option<Token>,
}

/// A timer-parked coroutine: awaiting a Task whose deadline is known
/// (e.g. `process.sleep_async(ms)`). Stored in a min-heap keyed by
/// `deadline`; polled only once the deadline has passed, avoiding O(n)
/// re-polling of all parked Tasks every tick.
struct TimerParked {
    deadline: std::time::Instant,
    coroutine: Coro,
    task: TaskRef,
}

// BinaryHeap is a max-heap by default; reverse comparison to get a
// min-heap so the earliest deadline is at the top.
impl PartialEq for TimerParked {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}
impl Eq for TimerParked {}
impl PartialOrd for TimerParked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TimerParked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed: earlier deadline = "greater" so it surfaces to the top.
        other.deadline.cmp(&self.deadline)
    }
}

/// Which kind of I/O source is registered with mio.
enum IoSource {
    Stream(mio::net::TcpStream),
    Listener(mio::net::TcpListener),
}

/// A registered I/O source, kept alive to maintain the mio registration.
struct IoEntry {
    source: IoSource,
    task: TaskRef,
}

/// The async scheduler. Holds a mio::Poll for event-driven I/O multiplexing.
pub struct Scheduler {
    poll: Poll,
    events: Events,
    next_token: usize,
    /// Token → I/O source + task. Keeps the mio source alive.
    io_entries: HashMap<Token, IoEntry>,
    /// TaskRef raw pointer → Token. Used to find the io_token when a
    /// coroutine awaits a Task.
    task_ptr_to_token: HashMap<usize, Token>,
    ready: Vec<(Coro, CoroInput)>,
    parked: Vec<Parked>,
    /// Timer-parked coroutines awaiting Tasks with a known deadline
    /// (e.g. `process.sleep_async`). Min-heap keyed by deadline; the top
    /// element is the next timer to fire. This avoids scanning all parked
    /// Tasks every tick when a timer is in flight.
    timer_parked: std::collections::BinaryHeap<TimerParked>,
    results: Vec<Result<Value, InterpreterError>>,
}

impl Scheduler {
    pub fn new() -> Self {
        let poll = Poll::new().expect("failed to create mio::Poll");
        Scheduler {
            poll,
            events: Events::with_capacity(4096),
            next_token: 1, // Token(0) reserved
            io_entries: HashMap::new(),
            task_ptr_to_token: HashMap::new(),
            ready: Vec::new(),
            parked: Vec::new(),
            timer_parked: std::collections::BinaryHeap::new(),
            results: Vec::new(),
        }
    }

    /// Register a TCP stream for readable events.
    /// Clones the stream (via try_clone) so the original remains usable.
    fn register_stream_readable(&mut self, stream: &std::net::TcpStream, task: TaskRef) -> Option<Token> {
        let cloned = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return None,
        };
        let mut mio_stream = mio::net::TcpStream::from_std(cloned);
        let token = Token(self.next_token);
        self.next_token += 1;
        if self.poll.registry().register(&mut mio_stream, token, Interest::READABLE).is_err() {
            return None;
        }
        let task_ptr = Rc::as_ptr(&task) as usize;
        self.io_entries.insert(token, IoEntry { source: IoSource::Stream(mio_stream), task });
        self.task_ptr_to_token.insert(task_ptr, token);
        Some(token)
    }

    /// Register a TCP listener for readable events (incoming connections).
    /// Clones the listener (via try_clone) so the original remains usable.
    fn register_listener_readable(&mut self, listener: &std::net::TcpListener, task: TaskRef) -> Option<Token> {
        let cloned = match listener.try_clone() {
            Ok(l) => l,
            Err(_) => return None,
        };
        let mut mio_listener = mio::net::TcpListener::from_std(cloned);
        let token = Token(self.next_token);
        self.next_token += 1;
        if self.poll.registry().register(&mut mio_listener, token, Interest::READABLE).is_err() {
            return None;
        }
        let task_ptr = Rc::as_ptr(&task) as usize;
        self.io_entries.insert(token, IoEntry { source: IoSource::Listener(mio_listener), task });
        self.task_ptr_to_token.insert(task_ptr, token);
        Some(token)
    }

    /// Deregister an I/O source (stream or listener) and remove all associated mappings.
    fn deregister_stream(&mut self, token: Token) {
        if let Some(entry) = self.io_entries.remove(&token) {
            match entry.source {
                IoSource::Stream(mut s) => {
                    let _ = self.poll.registry().deregister(&mut s);
                }
                IoSource::Listener(mut l) => {
                    let _ = self.poll.registry().deregister(&mut l);
                }
            }
            let task_ptr = Rc::as_ptr(&entry.task) as usize;
            self.task_ptr_to_token.remove(&task_ptr);
        }
    }

    /// Find the io_token for a given TaskRef, if registered.
    fn find_io_token(&self, task: &TaskRef) -> Option<Token> {
        let task_ptr = Rc::as_ptr(task) as usize;
        self.task_ptr_to_token.get(&task_ptr).copied()
    }

    /// Spawn a coroutine that runs `body`.
    pub fn spawn_handler<F>(&mut self, body: F)
    where
        F: FnOnce() -> Result<Value, InterpreterError> + 'static,
    {
        // Use an explicit 8 MB stack (instead of DefaultStack ~128 KB) so
        // that handler bodies with moderate recursion (e.g. processing
        // nested data structures, memoized algorithms) don't overflow.
        // The tree-walking interpreter uses ~5-10 KB per recursion level,
        // so 8 MB supports ~800-1600 levels of recursion within a handler.
        let stack = corosensei::stack::DefaultStack::new(8 * 1024 * 1024)
            .unwrap_or_else(|_| corosensei::stack::DefaultStack::default());
        let coro: Coro = Coroutine::with_stack(
            stack,
            move |yielder: &corosensei::Yielder<CoroInput, YieldSignal>, _input: CoroInput| {
                let yielder_ptr: YielderPtr = yielder as *const _;
                CURRENT_YIELDER.with(|y| {
                    *y.borrow_mut() = yielder_ptr;
                });
                let result = body();
                CURRENT_YIELDER.with(|y| {
                    *y.borrow_mut() = core::ptr::null();
                });
                result
            },
        );
        self.ready.push((coro, None));
    }

    /// Run one scheduler tick.
    ///
    /// 1. Run all ready coroutines (until they yield or return).
    /// 2. Compute the mio poll timeout from the earliest timer deadline
    ///    (if any) and wait for I/O events.
    /// 3. Poll I/O Tasks whose mio token is ready, legacy combinator Tasks
    ///    (no deadline, no io_token) every tick, and timer Tasks whose
    ///    deadline has elapsed.
    /// 4. Resume coroutines whose Tasks are ready.
    pub fn tick(&mut self) -> bool {
        // 1. Run all ready coroutines.
        while let Some((mut coro, input)) = self.ready.pop() {
            match coro.resume(input) {
                CoroutineResult::Yield(YieldSignal::AwaitTask(task)) => {
                    let io_token = self.find_io_token(&task);
                    // Route the parked coroutine to the right queue:
                    //   - Task with a deadline hint → timer_parked (min-heap)
                    //   - Task with an I/O token    → parked (I/O wait)
                    //   - Otherwise (combinator)    → parked (poll every tick)
                    let deadline = {
                        let t = task.borrow();
                        match &*t {
                            crate::value::TaskState::Pending(_, Some(d)) => Some(*d),
                            _ => None,
                        }
                    };
                    if let Some(d) = deadline {
                        self.timer_parked.push(TimerParked {
                            deadline: d,
                            coroutine: coro,
                            task,
                        });
                    } else {
                        self.parked.push(Parked { coroutine: coro, task, io_token });
                    }
                }
                CoroutineResult::Return(result) => {
                    self.results.push(result);
                }
            }
        }

        // 2. Compute mio poll timeout from the earliest timer deadline.
        let now = std::time::Instant::now();
        let next_timer_deadline = self.timer_parked.peek().map(|p| p.deadline);
        let has_io_parked = self.parked.iter().any(|p| p.io_token.is_some());
        let has_unkeyed_parked = self.parked.iter().any(|p| p.io_token.is_none());

        // Timeout: sleep until the earliest timer deadline (so the timer
        // fires promptly), or up to 100ms if only I/O is parked (woken
        // early by mio on readiness), or 1ms for legacy combinator Tasks.
        // - If a timer is parked: sleep until its deadline (no cap — we
        //   WANT to wake precisely when it fires, even if that's 500ms
        //   away). mio will wake us earlier if an I/O event arrives.
        // - If only I/O is parked: up to 100ms.
        // - If only legacy combinator Tasks are parked: 1ms.
        // - If nothing is parked: no wait.
        let timeout = if self.parked.is_empty() && self.timer_parked.is_empty() {
            None
        } else if let Some(dl) = next_timer_deadline {
            if dl > now {
                Some(dl - now)
            } else {
                Some(std::time::Duration::from_millis(0))
            }
        } else if has_unkeyed_parked {
            Some(std::time::Duration::from_millis(1))
        } else {
            Some(std::time::Duration::from_millis(100))
        };

        if has_io_parked || has_unkeyed_parked || next_timer_deadline.is_some() {
            let _ = self.poll.poll(&mut self.events, timeout);
        }

        // Collect ready tokens.
        let ready_tokens: HashSet<Token> = self.events.iter().map(|e| e.token()).collect();
        self.events.clear();

        let mut completed_tokens = Vec::new();

        // 3a. Poll I/O and legacy combinator Tasks in `parked`.
        let mut still_parked = Vec::new();
        let parked = std::mem::take(&mut self.parked);
        for p in parked {
            // For io Tasks, only poll if mio reported the token ready.
            // For legacy combinator Tasks (io_token=None), poll every tick.
            let should_poll = match p.io_token {
                Some(token) => ready_tokens.contains(&token),
                None => true,
            };

            if !should_poll {
                still_parked.push(p);
                continue;
            }

            let ready_value = {
                let task_ref = p.task.borrow();
                match &*task_ref {
                    crate::value::TaskState::Ready(v) => Some(v.clone()),
                    crate::value::TaskState::Consumed => Some(Value::Nil),
                    crate::value::TaskState::Pending(f, _) => match f() {
                        TaskPoll::Ready(v) => Some(v),
                        TaskPoll::Pending => None,
                    },
                }
            };

            if let Some(v) = ready_value {
                *p.task.borrow_mut() = crate::value::TaskState::Consumed;
                if let Some(token) = p.io_token {
                    completed_tokens.push(token);
                }
                self.ready.push((p.coroutine, Some(ResumeSignal::TaskReady(v))));
            } else {
                still_parked.push(p);
            }
        }
        self.parked = still_parked;

        // 3b. Poll timer Tasks whose deadline has elapsed. The min-heap
        //    surfaces the earliest deadline at the top, so we pop and poll
        //    until the top's deadline is in the future.
        let now = std::time::Instant::now();
        while let Some(top) = self.timer_parked.peek() {
            if top.deadline > now {
                break;
            }
            let tp = self.timer_parked.pop().unwrap();
            let ready_value = {
                let task_ref = tp.task.borrow();
                match &*task_ref {
                    crate::value::TaskState::Ready(v) => Some(v.clone()),
                    crate::value::TaskState::Consumed => Some(Value::Nil),
                    crate::value::TaskState::Pending(f, _) => match f() {
                        TaskPoll::Ready(v) => Some(v),
                        TaskPoll::Pending => None,
                    },
                }
            };
            if let Some(v) = ready_value {
                *tp.task.borrow_mut() = crate::value::TaskState::Consumed;
                self.ready.push((tp.coroutine, Some(ResumeSignal::TaskReady(v))));
            } else {
                // Deadline elapsed but the Task's poll fn still returns
                // Pending. This can happen if the poll fn has a slightly
                // later internal deadline than the hint. Re-queue with the
                // same deadline + 1ms grace so we don't busy-loop.
                let new_deadline = now + std::time::Duration::from_millis(1);
                self.timer_parked.push(TimerParked {
                    deadline: new_deadline,
                    coroutine: tp.coroutine,
                    task: tp.task,
                });
            }
        }

        // 4. Cleanup completed io entries.
        for token in completed_tokens {
            self.deregister_stream(token);
        }

        self.ready.is_empty() && self.parked.is_empty() && self.timer_parked.is_empty()
    }

    /// Run until all coroutines complete.
    ///
    /// Sets the thread-local scheduler pointer so that `register_readable`
    /// can be called from within coroutines. The loop relies on `tick`'s
    /// internal `mio::poll` timeout to avoid busy-waiting: timer Tasks get
    /// re-polled every 1ms, I/O Tasks wait up to 100ms for events.
    ///
    /// NOTE: a previous version had aggressive "deadlock detection" that
    /// broke the loop while timer Tasks were still pending (e.g.
    /// `process.sleep_async(500)`). This caused `run_until_complete` to
    /// return with coroutines still parked and `CURRENT_YIELDER` still
    /// pointing at a suspended coroutine's yielder. Subsequent top-level
    /// `await` calls would then mistakenly take the coroutine path
    /// (`in_coroutine()` → true), invoke `await_task` on the frozen
    /// yielder, and trigger `suspend()` on an already-suspended stack —
    /// corrupting the coroutine stack and segfaulting. The fix is to keep
    /// ticking until all coroutines genuinely finish.
    pub fn run_until_complete(&mut self) -> Vec<Result<Value, InterpreterError>> {
        let self_ptr: *mut Scheduler = self;
        CURRENT_SCHEDULER.with(|s| {
            *s.borrow_mut() = self_ptr;
        });
        loop {
            let done = self.tick();
            if done {
                break;
            }
        }
        CURRENT_SCHEDULER.with(|s| {
            *s.borrow_mut() = core::ptr::null_mut();
        });
        // Defensive: clear CURRENT_YIELDER in case a coroutine is still
        // parked (shouldn't happen after the fix above, but guards against
        // future regressions causing the same segfault).
        CURRENT_YIELDER.with(|y| {
            *y.borrow_mut() = core::ptr::null();
        });
        std::mem::take(&mut self.results)
    }

    pub fn has_work(&self) -> bool {
        !self.ready.is_empty() || !self.parked.is_empty() || !self.timer_parked.is_empty()
    }

    /// True if there are coroutines ready to resume without waiting on
    /// I/O or a timer. Used by `drain_mailboxes_async` to decide whether
    /// to keep ticking (fast handlers still draining) or return control
    /// to the caller (only parked work remains).
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    /// True if there are timer-parked coroutines whose deadline hasn't
    /// fired yet. Used by `drain_mailboxes_async` to keep ticking (and
    /// thus let `mio::poll` block on the timer's deadline) so slow
    /// handlers resume within the same `yield` call instead of starving
    /// between accept-loop iterations.
    pub fn has_pending_timers(&self) -> bool {
        !self.timer_parked.is_empty()
    }

    pub fn parked_count(&self) -> usize {
        self.parked.len() + self.timer_parked.len()
    }

    /// Take ownership of completed coroutine results (drains the internal
    /// buffer). Used by `drain_mailboxes_async` when driving the scheduler
    /// tick-by-tick so it can propagate errors after each `yield`.
    pub fn take_results(&mut self) -> Vec<Result<Value, InterpreterError>> {
        std::mem::take(&mut self.results)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
