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
        match yielder.suspend(YieldSignal::AwaitTask(task)) {
            Some(ResumeSignal::TaskReady(v)) => v,
            None => Value::Nil,
        }
    })
}

/// Check if we're currently inside a coroutine.
pub fn in_coroutine() -> bool {
    CURRENT_YIELDER.with(|y| !y.borrow().is_null())
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
    /// — poll every tick.
    io_token: Option<Token>,
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
    results: Vec<Result<Value, InterpreterError>>,
}

impl Scheduler {
    pub fn new() -> Self {
        let poll = Poll::new().expect("failed to create mio::Poll");
        Scheduler {
            poll,
            events: Events::with_capacity(1024),
            next_token: 1, // Token(0) reserved
            io_entries: HashMap::new(),
            task_ptr_to_token: HashMap::new(),
            ready: Vec::new(),
            parked: Vec::new(),
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
        let coro: Coro = Coroutine::with_stack(
            corosensei::stack::DefaultStack::default(),
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
    /// 2. If there are parked coroutines waiting on I/O, call mio::Poll::poll
    ///    to wait for events. Timer-based Tasks (no io_token) are polled
    ///    every tick.
    /// 3. Poll parked coroutines whose I/O is ready or that are timer-based.
    /// 4. Resume coroutines whose Tasks are ready.
    pub fn tick(&mut self) -> bool {
        // 1. Run all ready coroutines.
        while let Some((mut coro, input)) = self.ready.pop() {
            match coro.resume(input) {
                CoroutineResult::Yield(YieldSignal::AwaitTask(task)) => {
                    let io_token = self.find_io_token(&task);
                    self.parked.push(Parked { coroutine: coro, task, io_token });
                }
                CoroutineResult::Return(result) => {
                    self.results.push(result);
                }
            }
        }

        // 2. Wait for I/O events if there are parked io Tasks.
        let has_io_parked = self.parked.iter().any(|p| p.io_token.is_some());
        let has_timer_parked = self.parked.iter().any(|p| p.io_token.is_none());

        // Timeout: if only io Tasks are parked, wait up to 100ms for events.
        // If timer Tasks are parked, use a short timeout (1ms) so timers
        // get re-polled frequently. If nothing is parked, no wait needed.
        let timeout = if self.parked.is_empty() {
            None
        } else if has_timer_parked {
            Some(std::time::Duration::from_millis(1))
        } else {
            Some(std::time::Duration::from_millis(100))
        };

        if has_io_parked || has_timer_parked {
            let _ = self.poll.poll(&mut self.events, timeout);
        }

        // Collect ready tokens.
        let ready_tokens: HashSet<Token> = self.events.iter().map(|e| e.token()).collect();
        self.events.clear();

        // 3. Poll parked coroutines' tasks.
        let mut still_parked = Vec::new();
        let parked = std::mem::take(&mut self.parked);
        let mut completed_tokens = Vec::new();

        for p in parked {
            // For io Tasks, only poll if mio reported the token ready.
            // For timer Tasks (io_token=None), poll every tick.
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
                    crate::value::TaskState::Pending(f) => match f() {
                        TaskPoll::Ready(v) => Some(v),
                        TaskPoll::Pending => None,
                    },
                }
            };

            if let Some(v) = ready_value {
                *p.task.borrow_mut() = crate::value::TaskState::Consumed;
                // Deregister io source if this was an io Task.
                if let Some(token) = p.io_token {
                    completed_tokens.push(token);
                }
                self.ready.push((p.coroutine, Some(ResumeSignal::TaskReady(v))));
            } else {
                still_parked.push(p);
            }
        }
        self.parked = still_parked;

        // 4. Cleanup completed io entries.
        for token in completed_tokens {
            self.deregister_stream(token);
        }

        self.ready.is_empty() && self.parked.is_empty()
    }

    /// Run until all coroutines complete or deadlock (all parked, none ready).
    /// Sets the thread-local scheduler pointer so that `register_readable`
    /// can be called from within coroutines.
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
            if self.ready.is_empty() && !self.parked.is_empty() {
                // Check for true deadlock: no ready coroutines, all parked.
                // Try one more tick in case mio events unblocked something.
                let done2 = self.tick();
                if done2 || (self.ready.is_empty() && !self.parked.is_empty()) {
                    break;
                }
            }
        }
        CURRENT_SCHEDULER.with(|s| {
            *s.borrow_mut() = core::ptr::null_mut();
        });
        std::mem::take(&mut self.results)
    }

    pub fn has_work(&self) -> bool {
        !self.ready.is_empty() || !self.parked.is_empty()
    }

    pub fn parked_count(&self) -> usize {
        self.parked.len()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
