//! `onely` — the implementation crate for the `1y` programming language.
//!
//! Phase 0: lexer, parser, AST, pretty-printer.
//! Phase 1: tree-walking interpreter with persistent collections.

pub mod ast;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod runtime;
pub mod value;

pub use ast::{Program, Span, Spanned};
pub use error::{ErrorReport, SourceError};
pub use interpreter::Interpreter;
pub use lexer::tokenize;
pub use parser::parse;
pub use value::{ActorPid, CrossEnvelope, SendValue, Value};
