//! `io` module — basic input/output (stdin/stdout/file).
//!
//! Exports:
//!   - `read_line() -> Str | Nil`      — read one line from stdin (Nil on EOF)
//!   - `read_to_string(path) -> Str`   — read an entire file
//!   - `write(path, content) -> Nil`   — write to a file (truncate)
//!   - `append(path, content) -> Nil`  — append to a file
//!   - `exists(path) -> Bool`          — check if a file/path exists

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};

pub fn build() -> ModuleRef {
    make_module(
        "io",
        &[
            ("read_line", NativeFn { name: "read_line", func: bi_read_line }),
            ("read_to_string", NativeFn { name: "read_to_string", func: bi_read_to_string }),
            ("write", NativeFn { name: "write", func: bi_write }),
            ("append", NativeFn { name: "append", func: bi_append }),
            ("exists", NativeFn { name: "exists", func: bi_exists }),
        ],
    )
}

fn str_at(args: &[Value], idx: usize, name: &str) -> Result<String, InterpreterError> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok((**s).clone()),
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Str",
            got: v.type_name(),
            op: name.into(),
            span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1,
            got: args.len(),
            callee: name.into(),
            span: None,
        }),
    }
}

fn bi_read_line(_args: &[Value]) -> Result<Value, InterpreterError> {
    // Flush stdout so any pending `print` (without newline) is visible before
    // we block on stdin. This makes interactive prompts like
    // `print("Name? "); let name = io.read_line();` work as expected.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(Value::Nil),
        Ok(_) => {
            // Strip the trailing newline (if any).
            if line.ends_with('\n') { line.pop(); }
            if line.ends_with('\r') { line.pop(); }
            Ok(Value::str(line))
        }
        Err(e) => Err(InterpreterError::RuntimeError {
            msg: format!("io.read_line: {}", e),
            span: None,
        }),
    }
}

fn bi_read_to_string(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_at(args, 0, "io.read_to_string")?;
    std::fs::read_to_string(&path).map(Value::str).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("io.read_to_string: {}", e),
        span: None,
    })
}

fn bi_write(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_at(args, 0, "io.write")?;
    let content = str_at(args, 1, "io.write")?;
    std::fs::write(&path, content).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("io.write: {}", e),
        span: None,
    })?;
    Ok(Value::Nil)
}

fn bi_append(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_at(args, 0, "io.append")?;
    let content = str_at(args, 1, "io.append")?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("io.append: {}", e),
            span: None,
        })?;
    f.write_all(content.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("io.append: {}", e),
        span: None,
    })?;
    Ok(Value::Nil)
}

fn bi_exists(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_at(args, 0, "io.exists")?;
    Ok(Value::Bool(std::path::Path::new(&path).exists()))
}
