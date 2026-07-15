//! `env` module — environment variable access.
//!
//! Exports:
//!   - `get(name) -> Str | Nil`  — read an env var; Nil if unset
//!   - `set(name, value)`        — set an env var
//!   - `unset(name)`             — remove an env var
//!   - `args() -> Vec<Str>`      — the program's CLI args (excluding argv[0])
//!   - `vars() -> Vec[Str, Str]` — all env vars as [key, value] pairs

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};

pub fn build() -> ModuleRef {
    make_module(
        "env",
        &[
            ("get", NativeFn { name: "get", func: bi_get }),
            ("set", NativeFn { name: "set", func: bi_set }),
            ("unset", NativeFn { name: "unset", func: bi_unset }),
            ("args", NativeFn { name: "args", func: bi_args }),
            ("vars", NativeFn { name: "vars", func: bi_vars }),
        ],
    )
}

fn str_arg(args: &[Value], idx: usize, name: &str) -> Result<String, InterpreterError> {
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

fn bi_get(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_arg(args, 0, "env.get")?;
    match std::env::var(&name) {
        Ok(v) => Ok(Value::str(v)),
        Err(_) => Ok(Value::Nil),
    }
}

fn bi_set(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_arg(args, 0, "env.set")?;
    let value = str_arg(args, 1, "env.set")?;
    std::env::set_var(&name, &value);
    Ok(Value::Nil)
}

fn bi_unset(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_arg(args, 0, "env.unset")?;
    std::env::remove_var(&name);
    Ok(Value::Nil)
}

fn bi_args(_args: &[Value]) -> Result<Value, InterpreterError> {
    let args: Vec<Value> = std::env::args()
        .skip(1)
        .map(Value::str)
        .collect();
    Ok(Value::vec(args))
}

fn bi_vars(_args: &[Value]) -> Result<Value, InterpreterError> {
    let pairs: Vec<Value> = std::env::vars()
        .map(|(k, v)| Value::vec(vec![Value::str(k), Value::str(v)]))
        .collect();
    Ok(Value::vec(pairs))
}
