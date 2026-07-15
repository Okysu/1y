//! Coroutine-based async scheduler for the `1y` interpreter.
//!
//! Uses [`corosensei`] stackful coroutines with 64KB stacks. Each actor
//! handler runs in its own coroutine; `await` suspends and the scheduler
//! runs other ready coroutines.
//!
//! ## Architecture: thread-local yielder
//!
//! The interpreter's `eval_expr` is a deeply recursive function. When it
//! hits `Expr::Await`, it needs to suspend the current coroutine. But the
//! `corosensei::Yielder` is only available inside the coroutine closure.
//!
//! Solution: store the yielder callback in a `thread_local!`. When a coroutine
//! starts, it sets the thread-local yielder; when `eval_expr` calls
//! `await_task(task)`, it reads the thread-local yielder and suspends.

use crate::interpreter::error::InterpreterError;
use crate::value::{TaskPoll, TaskRef, Value};
use corosensei::{Coroutine, CoroutineResult};
use std::cell::RefCell;

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
// Scheduler
// ---------------------------------------------------------------------------

type Coro = Coroutine<CoroInput, YieldSignal, Result<Value, InterpreterError>>;

struct Parked {
    coroutine: Coro,
    task: TaskRef,
}

/// The async scheduler.
pub struct Scheduler {
    ready: Vec<(Coro, CoroInput)>,
    parked: Vec<Parked>,
    results: Vec<Result<Value, InterpreterError>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            ready: Vec::new(),
            parked: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Spawn a coroutine that runs `body`.
    pub fn spawn_handler<F>(&mut self, body: F)
    where
        F: FnOnce() -> Result<Value, InterpreterError> + 'static,
    {
        // corosensei's closure signature: FnOnce(&Yielder<Input, Yield>, Input) -> Return
        let coro: Coro = Coroutine::with_stack(
            corosensei::stack::DefaultStack::default(),
            move |yielder: &corosensei::Yielder<CoroInput, YieldSignal>, _input: CoroInput| {
                // Install the thread-local yielder pointer.
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
    pub fn tick(&mut self) -> bool {
        // 1. Run all ready coroutines.
        while let Some((mut coro, input)) = self.ready.pop() {
            match coro.resume(input) {
                CoroutineResult::Yield(YieldSignal::AwaitTask(task)) => {
                    self.parked.push(Parked { coroutine: coro, task });
                }
                CoroutineResult::Return(result) => {
                    self.results.push(result);
                }
            }
        }

        // 2. Poll parked coroutines' tasks.
        let mut still_parked = Vec::new();
        let parked = std::mem::take(&mut self.parked);
        for p in parked {
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
                self.ready
                    .push((p.coroutine, Some(ResumeSignal::TaskReady(v))));
            } else {
                still_parked.push(p);
            }
        }
        self.parked = still_parked;

        self.ready.is_empty() && self.parked.is_empty()
    }

    /// Run until all coroutines complete or deadlock.
    pub fn run_until_complete(&mut self) -> Vec<Result<Value, InterpreterError>> {
        loop {
            let done = self.tick();
            if done {
                break;
            }
            if self.ready.is_empty() && !self.parked.is_empty() {
                break;
            }
        }
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
