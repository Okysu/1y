//! Stack-based bytecode VM for the 1y language.
//!
//! Replaces the tree-walking interpreter's deep native recursion with a
//! `loop { dispatch(op) }` cycle. Each 1y function call pushes a [`Frame`]
//! onto a heap-allocated call stack — **no native Rust recursion is used for
//! 1y-level calls** — so recursion depth is bounded by heap memory, not the
//! OS stack. This is what lets `fib_memo(10000)` run without overflow.
//!
//! # Module layout
//!
//! - [`chunk`]: opcode enum + chunk structure
//! - [`closure`]: VM closures + Lua-style upvalues
//! - [`vm`]: the execution engine (frames, value stack, dispatch loop)
//! - [`compiler`]: AST → chunk compiler (separate module)

pub mod chunk;
pub mod closure;
pub mod vm;

pub use chunk::{Chunk, ExceptionHandler, OpCode};
pub use closure::{ClosureVm, Upvalue, UpvalueRef};
pub use vm::{Frame, MatchOutcome, Vm, VmCtx};
