//! `process` module — process control and command execution.
//!
//! Exports:
//!   - `exit(code)`                       — exit the program with a code
//!   - `exec(cmd, args) -> Str`           — run a command, return stdout as Str
//!   - `exec_status(cmd, args) -> Int`    — run a command, return exit code
//!   - `pid() -> Int`                     — current process ID
//!   - `cwd() -> Str`                     — current working directory
//!   - `set_cwd(path)`                    — change working directory
//!   - `sleep_ms(ms)`                     — sleep for N milliseconds

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};

pub fn build() -> ModuleRef {
    make_module(
        "process",
        &[
            ("exit", NativeFn { name: "exit", func: bi_exit }),
            ("exec", NativeFn { name: "exec", func: bi_exec }),
            ("exec_status", NativeFn { name: "exec_status", func: bi_exec_status }),
            ("pid", NativeFn { name: "pid", func: bi_pid }),
            ("cwd", NativeFn { name: "cwd", func: bi_cwd }),
            ("set_cwd", NativeFn { name: "set_cwd", func: bi_set_cwd }),
            ("sleep_ms", NativeFn { name: "sleep_ms", func: bi_sleep_ms }),
        ],
    )
}

fn str_at(args: &[Value], idx: usize, name: &str) -> Result<String, InterpreterError> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok((**s).clone()),
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Str", got: v.type_name(), op: name.into(), span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    }
}

fn vec_of_str_at(args: &[Value], idx: usize, name: &str) -> Result<Vec<String>, InterpreterError> {
    match args.get(idx) {
        Some(Value::Vec(v)) => {
            let mut out = Vec::with_capacity(v.len());
            for item in v.iter() {
                match item {
                    Value::Str(s) => out.push((**s).clone()),
                    _ => return Err(InterpreterError::TypeError {
                        expected: "Vec<Str>", got: item.type_name(), op: name.into(), span: None,
                    }),
                }
            }
            Ok(out)
        }
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Vec", got: v.type_name(), op: name.into(), span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    }
}

fn bi_exit(args: &[Value]) -> Result<Value, InterpreterError> {
    let code = match args.first() {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_i32(n).unwrap_or(0),
        _ => 0,
    };
    std::process::exit(code);
}

fn bi_exec(args: &[Value]) -> Result<Value, InterpreterError> {
    let cmd = str_at(args, 0, "process.exec")?;
    let cmd_args = vec_of_str_at(args, 1, "process.exec")?;
    let output = std::process::Command::new(&cmd)
        .args(&cmd_args)
        .output()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("process.exec: {}", e), span: None,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(Value::str(stdout))
}

fn bi_exec_status(args: &[Value]) -> Result<Value, InterpreterError> {
    let cmd = str_at(args, 0, "process.exec_status")?;
    let cmd_args = vec_of_str_at(args, 1, "process.exec_status")?;
    let status = std::process::Command::new(&cmd)
        .args(&cmd_args)
        .status()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("process.exec_status: {}", e), span: None,
        })?;
    Ok(Value::int(status.code().unwrap_or(-1)))
}

fn bi_pid(_args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::int(std::process::id() as i64))
}

fn bi_cwd(_args: &[Value]) -> Result<Value, InterpreterError> {
    std::env::current_dir()
        .map(|p| Value::str(p.display().to_string()))
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("process.cwd: {}", e), span: None,
        })
}

fn bi_set_cwd(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_at(args, 0, "process.set_cwd")?;
    std::env::set_current_dir(&path).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("process.set_cwd: {}", e), span: None,
    })?;
    Ok(Value::Nil)
}

fn bi_sleep_ms(args: &[Value]) -> Result<Value, InterpreterError> {
    let ms = match args.first() {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_u64(n).unwrap_or(0),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "process.sleep_ms".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "process.sleep_ms".into(), span: None,
        }),
    };
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Ok(Value::Nil)
}
