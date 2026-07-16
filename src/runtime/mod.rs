//! Async runtime (Phase 4.7: Zig-style colorless async).
//!
//! Built on [`corosensei`] stackful coroutines. Each actor handler runs in its
//! own coroutine; `await` suspends the coroutine until the awaited [`Task`]
//! is ready, and the scheduler runs other ready coroutines in the meantime.
//!
//! ## Design
//!
//! - **Single-threaded**: all coroutines run on the interpreter's thread.
//!   `Rc<RefCell<...>>` stays safe; no `Arc<Mutex>` needed.
//! - **Cooperative**: coroutines yield only at `await` points (or on completion).
//! - **Colorless**: any function can use `await` — there is no `async fn`
//!   marker, no function coloring. A function that never awaits runs exactly
//!   like a plain synchronous call.
//! - **Task-based**: async I/O functions (e.g. `socket.read_async`) return a
//!   `Value::Task` wrapping a poll function. `await` polls the task; if
//!   pending, the coroutine suspends.

pub mod scheduler;
pub mod registry;
pub mod worker;
