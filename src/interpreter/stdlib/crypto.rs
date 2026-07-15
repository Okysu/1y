//! `crypto` module — cryptographic primitives (hashing, HMAC, encoding, CSPRNG).
//!
//! Uses the `rand` crate's `OsRng` for cryptographically-secure randomness
//! (unlike the `random` module's xorshift, which is NOT secure). Hashing uses
//! the RustCrypto families (`sha2`, `sha1`, `md-5`, `hmac`).
//!
//! Data model: inputs are `Str` (UTF-8 bytes); hash/HMAC outputs are hex-encoded
//! `Str`; `random_bytes` returns a `Vec<Int>` of 0–255 values so callers can
//! index/transform them. `base64`/`hex` round-trip through `Str`.
//!
//! Exports:
//!   - `sha256(s) -> Str`           — SHA-256, hex output
//!   - `sha512(s) -> Str`           — SHA-512, hex output
//!   - `sha1(s) -> Str`             — SHA-1, hex output
//!   - `md5(s) -> Str`              — MD5, hex output
//!   - `hmac_sha256(key, s) -> Str` — HMAC-SHA-256, hex output
//!   - `hmac_sha512(key, s) -> Str` — HMAC-SHA-512, hex output
//!   - `base64_encode(s) -> Str`    — base64 encode
//!   - `base64_decode(s) -> Str`    — base64 decode (raw bytes as Str)
//!   - `hex_encode(s) -> Str`       — hex encode
//!   - `hex_decode(s) -> Str`       — hex decode (raw bytes as Str)
//!   - `random_bytes(n) -> Vec<Int>`— n cryptographically-secure random bytes
//!   - `secure_int(max) -> Int`     — random Int in [0, max)
//!   - `secure_float() -> Decimal`  — random Decimal in [0, 1)

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};
use bigdecimal::BigDecimal;
use hmac::{Hmac, Mac};
use md5::Md5;
use num_traits::{FromPrimitive, ToPrimitive};
use rand::{Rng, RngCore};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

pub fn build() -> ModuleRef {
    make_module(
        "crypto",
        &[
            ("sha256", NativeFn { name: "sha256", func: bi_sha256 }),
            ("sha512", NativeFn { name: "sha512", func: bi_sha512 }),
            ("sha1", NativeFn { name: "sha1", func: bi_sha1 }),
            ("md5", NativeFn { name: "md5", func: bi_md5 }),
            ("hmac_sha256", NativeFn { name: "hmac_sha256", func: bi_hmac_sha256 }),
            ("hmac_sha512", NativeFn { name: "hmac_sha512", func: bi_hmac_sha512 }),
            ("base64_encode", NativeFn { name: "base64_encode", func: bi_base64_encode }),
            ("base64_decode", NativeFn { name: "base64_decode", func: bi_base64_decode }),
            ("hex_encode", NativeFn { name: "hex_encode", func: bi_hex_encode }),
            ("hex_decode", NativeFn { name: "hex_decode", func: bi_hex_decode }),
            ("random_bytes", NativeFn { name: "random_bytes", func: bi_random_bytes }),
            ("secure_int", NativeFn { name: "secure_int", func: bi_secure_int }),
            ("secure_float", NativeFn { name: "secure_float", func: bi_secure_float }),
        ],
    )
}

// --- helpers ---

fn str_arg<'a>(args: &'a [Value], idx: usize, name: &str) -> Result<&'a str, InterpreterError> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok(s),
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

fn hex_str(bytes: &[u8]) -> Value {
    Value::str(hex::encode(bytes))
}

// --- hashes ---

fn bi_sha256(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "sha256")?;
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    Ok(hex_str(&h.finalize()))
}

fn bi_sha512(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "sha512")?;
    let mut h = Sha512::new();
    h.update(s.as_bytes());
    Ok(hex_str(&h.finalize()))
}

fn bi_sha1(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "sha1")?;
    let mut h = Sha1::new();
    h.update(s.as_bytes());
    Ok(hex_str(&h.finalize()))
}

fn bi_md5(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "md5")?;
    let mut h = Md5::new();
    h.update(s.as_bytes());
    Ok(hex_str(&h.finalize()))
}

// --- HMAC ---

fn bi_hmac_sha256(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "hmac_sha256".into(), span: None,
        });
    }
    let key = str_arg(args, 0, "hmac_sha256")?;
    let msg = str_arg(args, 1, "hmac_sha256")?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|_| InterpreterError::RuntimeError {
            msg: "hmac_sha256: invalid key length".into(),
            span: None,
        })?;
    mac.update(msg.as_bytes());
    Ok(hex_str(&mac.finalize().into_bytes()))
}

fn bi_hmac_sha512(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "hmac_sha512".into(), span: None,
        });
    }
    let key = str_arg(args, 0, "hmac_sha512")?;
    let msg = str_arg(args, 1, "hmac_sha512")?;
    type HmacSha512 = Hmac<Sha512>;
    let mut mac = HmacSha512::new_from_slice(key.as_bytes())
        .map_err(|_| InterpreterError::RuntimeError {
            msg: "hmac_sha512: invalid key length".into(),
            span: None,
        })?;
    mac.update(msg.as_bytes());
    Ok(hex_str(&mac.finalize().into_bytes()))
}

// --- base64 / hex ---

fn bi_base64_encode(args: &[Value]) -> Result<Value, InterpreterError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let s = str_arg(args, 0, "base64_encode")?;
    Ok(Value::str(STANDARD.encode(s.as_bytes())))
}

fn bi_base64_decode(args: &[Value]) -> Result<Value, InterpreterError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let s = str_arg(args, 0, "base64_decode")?;
    let bytes = STANDARD.decode(s)
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("base64_decode: {}", e),
            span: None,
        })?;
    // Raw bytes may not be valid UTF-8; lossy-convert to keep the Str type.
    Ok(Value::str(String::from_utf8_lossy(&bytes)))
}

fn bi_hex_encode(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "hex_encode")?;
    Ok(hex_str(s.as_bytes()))
}

fn bi_hex_decode(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = str_arg(args, 0, "hex_decode")?;
    let bytes = hex::decode(s)
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("hex_decode: {}", e),
            span: None,
        })?;
    Ok(Value::str(String::from_utf8_lossy(&bytes)))
}

// --- CSPRNG ---

fn bi_random_bytes(args: &[Value]) -> Result<Value, InterpreterError> {
    let n = match args.get(0) {
        Some(Value::Int(n)) => n,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "random_bytes".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "random_bytes".into(), span: None,
        }),
    };
    let count = n.to_u64().ok_or_else(|| InterpreterError::RuntimeError {
        msg: "random_bytes: count must be a non-negative integer".into(),
        span: None,
    })? as usize;
    let mut buf = vec![0u8; count];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let vals: Vec<Value> = buf.iter().map(|&b| Value::int(b as i64)).collect();
    Ok(Value::vec(vals))
}

fn bi_secure_int(args: &[Value]) -> Result<Value, InterpreterError> {
    let max = match args.get(0) {
        Some(Value::Int(n)) => n.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "secure_int".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "secure_int".into(), span: None,
        }),
    };
    if max <= num_bigint::BigInt::from(0) {
        return Err(InterpreterError::RuntimeError {
            msg: "secure_int: max must be > 0".into(),
            span: None,
        });
    }
    let max_u64 = max.to_u64().ok_or_else(|| InterpreterError::RuntimeError {
        msg: "secure_int: max too large".into(),
        span: None,
    })?;
    let r: u64 = rand::rngs::OsRng.gen_range(0..max_u64);
    Ok(Value::int(r as i64))
}

fn bi_secure_float(_args: &[Value]) -> Result<Value, InterpreterError> {
    let r: f64 = rand::rngs::OsRng.gen_range(0.0..1.0);
    match BigDecimal::from_f64(r) {
        Some(d) => Ok(Value::Decimal(d)),
        None => Err(InterpreterError::RuntimeError {
            msg: "secure_float: conversion failed".into(),
            span: None,
        }),
    }
}
