//! `bytes` module — immutable byte buffers.
//!
//! Exports:
//!   - `from_vec(vec) -> Bytes`       — build from a Vec<Int> (each Int in 0..=255)
//!   - `to_vec(b) -> Vec<Int>`        — convert back to a Vec<Int>
//!   - `len(b) -> Int`                — number of bytes
//!   - `get(b, i) -> Int`             — byte at index i (0-based); error if out of range
//!   - `slice(b, start, end) -> Bytes` — sub-buffer [start, end)
//!   - `concat(a, b) -> Bytes`        — concatenate two byte buffers
//!   - `push(b, byte) -> Bytes`       — return a new buffer with byte appended
//!   - `from_str(s) -> Bytes`         — UTF-8 encode a string
//!   - `to_str(b) -> Str`             — UTF-8 decode (error if invalid)
//!   - `from_hex(s) -> Bytes`         — decode a hex string
//!   - `to_hex(b) -> Str`             — encode as lowercase hex
//!
//! `Bytes` is immutable: every "modifying" operation returns a fresh `Bytes`.
//! Cheap to clone (Rc-shared). Suitable for building chunk streams and
//! parsing binary formats in 1y.

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};

pub fn build() -> ModuleRef {
    make_module(
        "bytes",
        &[
            ("from_vec", NativeFn { name: "from_vec", func: bi_from_vec }),
            ("to_vec", NativeFn { name: "to_vec", func: bi_to_vec }),
            ("len", NativeFn { name: "len", func: bi_len }),
            ("get", NativeFn { name: "get", func: bi_get }),
            ("slice", NativeFn { name: "slice", func: bi_slice }),
            ("concat", NativeFn { name: "concat", func: bi_concat }),
            ("push", NativeFn { name: "push", func: bi_push }),
            ("from_str", NativeFn { name: "from_str", func: bi_from_str }),
            ("to_str", NativeFn { name: "to_str", func: bi_to_str }),
            ("from_hex", NativeFn { name: "from_hex", func: bi_from_hex }),
            ("to_hex", NativeFn { name: "to_hex", func: bi_to_hex }),
        ],
    )
}

// --- helpers ---

fn as_bytes<'a>(v: &'a Value, op: &str) -> Result<&'a [u8], InterpreterError> {
    match v {
        Value::Bytes(b) => Ok(b.as_slice()),
        other => Err(InterpreterError::TypeError {
            expected: "Bytes",
            got: other.type_name(),
            op: op.into(),
            span: None,
        }),
    }
}

fn as_str<'a>(v: &'a Value, op: &str) -> Result<&'a str, InterpreterError> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        other => Err(InterpreterError::TypeError {
            expected: "String",
            got: other.type_name(),
            op: op.into(),
            span: None,
        }),
    }
}

fn as_usize(v: &Value, op: &str) -> Result<usize, InterpreterError> {
    match v {
        Value::Int(n) => num_traits::ToPrimitive::to_usize(n).ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("{}: integer out of usize range", op),
            span: None,
        }),
        other => Err(InterpreterError::TypeError {
            expected: "Int",
            got: other.type_name(),
            op: op.into(),
            span: None,
        }),
    }
}

/// Extract a single byte (0..=255) from a Value::Int, erroring on out-of-range.
fn as_byte(v: &Value, op: &str) -> Result<u8, InterpreterError> {
    match v {
        Value::Int(n) => {
            let i = num_traits::ToPrimitive::to_i64(n).ok_or_else(|| InterpreterError::RuntimeError {
                msg: format!("{}: byte value out of range", op),
                span: None,
            })?;
            if !(0..=255).contains(&i) {
                return Err(InterpreterError::RuntimeError {
                    msg: format!("{}: byte value {} not in 0..=255", op, i),
                    span: None,
                });
            }
            Ok(i as u8)
        }
        other => Err(InterpreterError::TypeError {
            expected: "Int",
            got: other.type_name(),
            op: op.into(),
            span: None,
        }),
    }
}

// --- builtins ---

fn bi_from_vec(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = match args.first() {
        Some(Value::Vec(v)) => v,
        Some(other) => return Err(InterpreterError::TypeError {
            expected: "Vec",
            got: other.type_name(),
            op: "bytes.from_vec".into(),
            span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "bytes.from_vec".into(), span: None,
        }),
    };
    let mut out = Vec::with_capacity(v.len());
    for (i, item) in v.iter().enumerate() {
        let b = as_byte(item, "bytes.from_vec").map_err(|e| match e {
            InterpreterError::RuntimeError { msg, .. } => InterpreterError::RuntimeError {
                msg: format!("bytes.from_vec: element {} invalid: {}", i, msg),
                span: None,
            },
            other => other,
        })?;
        out.push(b);
    }
    Ok(Value::bytes(out))
}

fn bi_to_vec(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = match args.first() {
        Some(v) => v,
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "bytes.to_vec".into(), span: None,
        }),
    };
    let bs = as_bytes(b, "bytes.to_vec")?;
    let items: Vec<Value> = bs.iter().map(|&x| Value::int(x as i64)).collect();
    Ok(Value::vec(items))
}

fn bi_len(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = match args.first() {
        Some(v) => v,
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "bytes.len".into(), span: None,
        }),
    };
    let bs = as_bytes(b, "bytes.len")?;
    Ok(Value::int(bs.len()))
}

fn bi_get(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 0, callee: "bytes.get".into(), span: None,
    })?;
    let idx_v = args.get(1).ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 1, callee: "bytes.get".into(), span: None,
    })?;
    let bs = as_bytes(b, "bytes.get")?;
    let i = as_usize(idx_v, "bytes.get")?;
    let x = bs.get(i).copied().ok_or_else(|| InterpreterError::RuntimeError {
        msg: format!("bytes.get: index {} out of range (len={})", i, bs.len()),
        span: None,
    })?;
    Ok(Value::int(x as i64))
}

fn bi_slice(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 3, got: 0, callee: "bytes.slice".into(), span: None,
    })?;
    let start_v = args.get(1).ok_or_else(|| InterpreterError::ArityError {
        expected: 3, got: 1, callee: "bytes.slice".into(), span: None,
    })?;
    let end_v = args.get(2).ok_or_else(|| InterpreterError::ArityError {
        expected: 3, got: 2, callee: "bytes.slice".into(), span: None,
    })?;
    let bs = as_bytes(b, "bytes.slice")?;
    let start = as_usize(start_v, "bytes.slice")?;
    let end = as_usize(end_v, "bytes.slice")?;
    if start > end || end > bs.len() {
        return Err(InterpreterError::RuntimeError {
            msg: format!(
                "bytes.slice: range [{}, {}) out of bounds (len={})",
                start, end, bs.len()
            ),
            span: None,
        });
    }
    Ok(Value::bytes(bs[start..end].to_vec()))
}

fn bi_concat(args: &[Value]) -> Result<Value, InterpreterError> {
    let a = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 0, callee: "bytes.concat".into(), span: None,
    })?;
    let b = args.get(1).ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 1, callee: "bytes.concat".into(), span: None,
    })?;
    let a_bs = as_bytes(a, "bytes.concat")?;
    let b_bs = as_bytes(b, "bytes.concat")?;
    let mut out = Vec::with_capacity(a_bs.len() + b_bs.len());
    out.extend_from_slice(a_bs);
    out.extend_from_slice(b_bs);
    Ok(Value::bytes(out))
}

fn bi_push(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 0, callee: "bytes.push".into(), span: None,
    })?;
    let byte_v = args.get(1).ok_or_else(|| InterpreterError::ArityError {
        expected: 2, got: 1, callee: "bytes.push".into(), span: None,
    })?;
    let bs = as_bytes(b, "bytes.push")?;
    let x = as_byte(byte_v, "bytes.push")?;
    let mut out = Vec::with_capacity(bs.len() + 1);
    out.extend_from_slice(bs);
    out.push(x);
    Ok(Value::bytes(out))
}

fn bi_from_str(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 1, got: 0, callee: "bytes.from_str".into(), span: None,
    })?;
    let s = as_str(s, "bytes.from_str")?;
    Ok(Value::bytes(s.as_bytes().to_vec()))
}

fn bi_to_str(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 1, got: 0, callee: "bytes.to_str".into(), span: None,
    })?;
    let bs = as_bytes(b, "bytes.to_str")?;
    match String::from_utf8(bs.to_vec()) {
        Ok(s) => Ok(Value::str(s)),
        Err(_) => Err(InterpreterError::RuntimeError {
            msg: "bytes.to_str: invalid UTF-8".into(),
            span: None,
        }),
    }
}

fn bi_from_hex(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 1, got: 0, callee: "bytes.from_hex".into(), span: None,
    })?;
    let s = as_str(s, "bytes.from_hex")?;
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() % 2 != 0 {
        return Err(InterpreterError::RuntimeError {
            msg: "bytes.from_hex: odd-length hex string".into(),
            span: None,
        });
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_digit(bytes[i]).ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("bytes.from_hex: invalid hex digit '{}'", bytes[i] as char),
            span: None,
        })?;
        let lo = hex_digit(bytes[i + 1]).ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("bytes.from_hex: invalid hex digit '{}'", bytes[i + 1] as char),
            span: None,
        })?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(Value::bytes(out))
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn bi_to_hex(args: &[Value]) -> Result<Value, InterpreterError> {
    let b = args.first().ok_or_else(|| InterpreterError::ArityError {
        expected: 1, got: 0, callee: "bytes.to_hex".into(), span: None,
    })?;
    let bs = as_bytes(b, "bytes.to_hex")?;
    let mut out = String::with_capacity(bs.len() * 2);
    for &x in bs {
        out.push(char::from_digit((x >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((x & 0x0f) as u32, 16).unwrap());
    }
    Ok(Value::str(out))
}
