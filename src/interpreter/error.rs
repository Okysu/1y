//! Runtime errors for the `1y` interpreter.
//!
//! Every eval function returns `Result<T, InterpreterError>`. The language's
//! `raise`/`try`/`rescue` mechanism is layered on top: a `raise` produces
//! [`InterpreterError::UserException`], and `try`/`rescue` catches it.

use crate::ast::Span;
use crate::value::Value;

/// A runtime error. Carries a span for tracebacks when available.
#[derive(Debug, Clone)]
pub enum InterpreterError {
    /// `raise <expr>` — a user-thrown exception with an arbitrary value.
    UserException { value: Value, span: Option<Span> },

    /// Referencing a name that is not in scope.
    NameError { name: String, span: Option<Span> },

    /// Wrong type for an operation (e.g. adding a string to a number).
    TypeError {
        expected: &'static str,
        got: &'static str,
        op: String,
        span: Option<Span>,
    },

    /// Wrong number of arguments to a function call.
    ArityError {
        expected: usize,
        got: usize,
        callee: String,
        span: Option<Span>,
    },

    /// Index out of bounds, key not found, etc.
    IndexError { msg: String, span: Option<Span> },

    /// A `match` expression failed to match any arm.
    PatternMatchFail { value: Value, span: Option<Span> },

    /// Division by zero.
    DivisionByZero { span: Option<Span> },

    /// A generic runtime error (used for misc conditions).
    RuntimeError { msg: String, span: Option<Span> },

    /// `return` / `reply` unwinding — internal control-flow signal, not a
    /// user-visible error. Caught by the enclosing function call.
    Return { value: Value, span: Option<Span> },
    Reply { value: Value, span: Option<Span> },
    /// `retry` inside a `transact` block — internal control-flow signal
    /// caught by the enclosing transaction. Aborts the current attempt and
    /// restarts the transaction body (up to a retry limit).
    Retry { span: Option<Span> },
    /// `break [expr]` inside a loop — internal control-flow signal caught by
    /// the enclosing `while`/`for`/`loop`. Carries the optional break value.
    Break { value: Option<Value>, span: Option<Span> },
    /// `continue` inside a loop — internal control-flow signal caught by the
    /// enclosing loop; skips to the next iteration.
    Continue { span: Option<Span> },

    /// Module import error (Phase 4): file not found, circular dependency,
    /// missing export, etc.
    ImportError { path: String, msg: String, span: Option<Span> },
}

impl InterpreterError {
    pub fn with_span(mut self, span: Span) -> Self {
        match &mut self {
            InterpreterError::UserException { span: s, .. }
            | InterpreterError::NameError { span: s, .. }
            | InterpreterError::TypeError { span: s, .. }
            | InterpreterError::ArityError { span: s, .. }
            | InterpreterError::IndexError { span: s, .. }
            | InterpreterError::PatternMatchFail { span: s, .. }
            | InterpreterError::DivisionByZero { span: s, .. }
            | InterpreterError::RuntimeError { span: s, .. }
            | InterpreterError::Return { span: s, .. }
            | InterpreterError::Reply { span: s, .. }
            | InterpreterError::Retry { span: s }
            | InterpreterError::Break { span: s, .. }
            | InterpreterError::Continue { span: s }
            | InterpreterError::ImportError { span: s, .. } => {
                if s.is_none() {
                    *s = Some(span);
                }
            }
        }
        self
    }

    /// True if this error is a user-thrown exception (catchable by `rescue`).
    pub fn is_user_exception(&self) -> bool {
        matches!(self, InterpreterError::UserException { .. })
    }

    /// Extract the exception value if this is a [`UserException`].
    pub fn as_exception_value(&self) -> Option<&Value> {
        match self {
            InterpreterError::UserException { value, .. } => Some(value),
            _ => None,
        }
    }
}

impl std::fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpreterError::UserException { value, .. } => {
                write!(f, "uncaught exception: {}", value)
            }
            InterpreterError::NameError { name, .. } => {
                write!(f, "name `{}` is not defined", name)
            }
            InterpreterError::TypeError {
                expected, got, op, ..
            } => {
                write!(f, "type error in `{}`: expected {}, got {}", op, expected, got)
            }
            InterpreterError::ArityError {
                expected, got, callee, ..
            } => {
                write!(f, "`{}` expects {} argument(s), got {}", callee, expected, got)
            }
            InterpreterError::IndexError { msg, .. } => write!(f, "index error: {}", msg),
            InterpreterError::PatternMatchFail { value, .. } => {
                write!(f, "no pattern matched {}", value)
            }
            InterpreterError::DivisionByZero { .. } => write!(f, "division by zero"),
            InterpreterError::RuntimeError { msg, .. } => write!(f, "runtime error: {}", msg),
            InterpreterError::Return { .. } => write!(f, "return"),
            InterpreterError::Reply { .. } => write!(f, "reply"),
            InterpreterError::Retry { .. } => write!(f, "retry"),
            InterpreterError::Break { .. } => write!(f, "break"),
            InterpreterError::Continue { .. } => write!(f, "continue"),
            InterpreterError::ImportError { path, msg, .. } => {
                write!(f, "import error: {}: {}", path, msg)
            }
        }
    }
}

impl std::error::Error for InterpreterError {}
