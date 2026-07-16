//! `parallel` module — user-facing multi-threading API.
//!
//! Provides `parallel.call`, `parallel.spawn`, `parallel.join`,
//! `parallel.map`, and `parallel.cores`. Built on the global `WorkerPool`.
//!
//! Functions are called by name (string) on worker threads that have
//! pre-loaded the entry file's definitions. Arguments and return values
//! must be `SendValue`-compatible (Int, Str, Bool, Nil, Vec, Map, Set,
//! Variant, Struct).

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::runtime::worker;
use crate::value::{ModuleRef, NativeFn, NativeResource, SendValue, Value};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Build the `parallel` standard library module.
pub fn build() -> ModuleRef {
    make_module(
        "parallel",
        &[
            ("cores", NativeFn { name: "cores", func: bi_cores }),
            ("call", NativeFn { name: "call", func: bi_call }),
            ("spawn", NativeFn { name: "spawn", func: bi_spawn }),
            ("join", NativeFn { name: "join", func: bi_join }),
            ("map", NativeFn { name: "parallel_map", func: bi_map }),
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

/// parallel.cores() -> Int — number of CPU cores available.
fn bi_cores(_args: &[Value]) -> Result<Value, InterpreterError> {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    Ok(Value::Int(n.into()))
}

/// parallel.call(func_name: Str, args: Vec) -> Value — synchronously call
/// a function on a worker thread and return its result.
fn bi_call(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_at(args, 0, "parallel.call")?;
    let arg_vec = match args.get(1) {
        Some(Value::Vec(v)) => v.clone(),
        Some(v) => {
            return Err(InterpreterError::TypeError {
                expected: "Vec",
                got: v.type_name(),
                op: "parallel.call".into(),
                span: None,
            })
        }
        None => {
            return Err(InterpreterError::ArityError {
                expected: 2,
                got: args.len(),
                callee: "parallel.call".into(),
                span: None,
            })
        }
    };

    let pool = worker::get_global_pool().ok_or(InterpreterError::RuntimeError {
        msg: "parallel pool not initialized (use `1y run` to enable multi-threading)".into(),
        span: None,
    })?;

    let send_args: Vec<SendValue> = arg_vec
        .iter()
        .map(SendValue::from_value)
        .collect::<Result<_, _>>()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("argument not Send: {}", e),
            span: None,
        })?;

    let rx = pool.call(name, send_args);
    let result = rx.recv().map_err(|e| InterpreterError::RuntimeError {
        msg: format!("parallel.call failed: {}", e),
        span: None,
    })?;

    result
        .map(|sv| sv.into_value())
        .map_err(|e| InterpreterError::RuntimeError { msg: e, span: None })
}

/// parallel.spawn(func_name: Str, args: Vec) -> Handle — asynchronously
/// call a function on a worker thread. Returns a handle for parallel.join.
fn bi_spawn(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_at(args, 0, "parallel.spawn")?;
    let arg_vec = match args.get(1) {
        Some(Value::Vec(v)) => v.clone(),
        Some(v) => {
            return Err(InterpreterError::TypeError {
                expected: "Vec",
                got: v.type_name(),
                op: "parallel.spawn".into(),
                span: None,
            })
        }
        None => {
            return Err(InterpreterError::ArityError {
                expected: 2,
                got: args.len(),
                callee: "parallel.spawn".into(),
                span: None,
            })
        }
    };

    let pool = worker::get_global_pool().ok_or(InterpreterError::RuntimeError {
        msg: "parallel pool not initialized".into(),
        span: None,
    })?;

    let send_args: Vec<SendValue> = arg_vec
        .iter()
        .map(SendValue::from_value)
        .collect::<Result<_, _>>()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("argument not Send: {}", e),
            span: None,
        })?;

    let rx = pool.call(name, send_args);

    Ok(Value::Opaque(Rc::new(NativeResource::ParallelHandle(
        Arc::new(Mutex::new(Some(rx))),
    ))))
}

/// parallel.join(handle) -> Value — wait for a spawned task to complete
/// and return its result.
fn bi_join(args: &[Value]) -> Result<Value, InterpreterError> {
    let handle = match args.get(0) {
        Some(Value::Opaque(r)) => r.clone(),
        Some(v) => {
            return Err(InterpreterError::TypeError {
                expected: "Handle",
                got: v.type_name(),
                op: "parallel.join".into(),
                span: None,
            })
        }
        None => {
            return Err(InterpreterError::ArityError {
                expected: 1,
                got: 0,
                callee: "parallel.join".into(),
                span: None,
            })
        }
    };

    match &*handle {
        NativeResource::ParallelHandle(rx_lock) => {
            let mut guard = rx_lock.lock().unwrap();
            let rx = guard.take().ok_or(InterpreterError::RuntimeError {
                msg: "handle already joined".into(),
                span: None,
            })?;
            let result = rx.recv().map_err(|e| InterpreterError::RuntimeError {
                msg: format!("parallel.join failed: {}", e),
                span: None,
            })?;
            result
                .map(|sv| sv.into_value())
                .map_err(|e| InterpreterError::RuntimeError { msg: e, span: None })
        }
        _ => Err(InterpreterError::TypeError {
            expected: "ParallelHandle",
            got: "other resource",
            op: "parallel.join".into(),
            span: None,
        }),
    }
}

/// parallel.map(func_name: Str, args_list: Vec<Vec>) -> Vec — call a
/// function in parallel for each argument set, returning all results.
fn bi_map(args: &[Value]) -> Result<Value, InterpreterError> {
    let name = str_at(args, 0, "parallel.map")?;
    let tasks = match args.get(1) {
        Some(Value::Vec(v)) => v.clone(),
        Some(v) => {
            return Err(InterpreterError::TypeError {
                expected: "Vec",
                got: v.type_name(),
                op: "parallel.map".into(),
                span: None,
            })
        }
        None => {
            return Err(InterpreterError::ArityError {
                expected: 2,
                got: args.len(),
                callee: "parallel.map".into(),
                span: None,
            })
        }
    };

    let pool = worker::get_global_pool().ok_or(InterpreterError::RuntimeError {
        msg: "parallel pool not initialized".into(),
        span: None,
    })?;

    // Submit all tasks.
    let mut receivers = Vec::new();
    for task_args in tasks.iter() {
        let inner = match task_args {
            Value::Vec(v) => v.clone(),
            v => {
                return Err(InterpreterError::TypeError {
                    expected: "Vec",
                    got: v.type_name(),
                    op: "parallel.map".into(),
                    span: None,
                })
            }
        };
        let send_args: Vec<SendValue> = inner
            .iter()
            .map(SendValue::from_value)
            .collect::<Result<_, _>>()
            .map_err(|e| InterpreterError::RuntimeError {
                msg: format!("argument not Send: {}", e),
                span: None,
            })?;
        let rx = pool.call(name.clone(), send_args);
        receivers.push(rx);
    }

    // Collect results.
    let mut results = Vec::new();
    for rx in receivers {
        let result = rx.recv().map_err(|e| InterpreterError::RuntimeError {
            msg: format!("parallel.map task failed: {}", e),
            span: None,
        })?;
        let value = result
            .map(|sv| sv.into_value())
            .map_err(|e| InterpreterError::RuntimeError { msg: e, span: None })?;
        results.push(value);
    }

    Ok(Value::vec(results))
}
