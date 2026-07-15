//! `random` module — pseudo-random number generation.
//!
//! Uses a thread-local xorshift PRNG seeded from system time. Not
//! cryptographically secure — for crypto use the `crypto` module (Phase 4.5).
//!
//! Exports:
//!   - `int(max) -> Int`              — random integer in [0, max)
//!   - `range(min, max) -> Int`       — random integer in [min, max)
//!   - `float() -> Decimal`           — random float in [0, 1)
//!   - `bool() -> Bool`               — random boolean
//!   - `pick(vec) -> Value`           — random element from a Vec
//!   - `shuffle(vec) -> Vec`          — shuffled copy of a Vec
//!   - `seed(n)`                      — re-seed the PRNG

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};
use bigdecimal::BigDecimal;
use num_traits::FromPrimitive;
use std::cell::Cell;

pub fn build() -> ModuleRef {
    make_module(
        "random",
        &[
            ("int", NativeFn { name: "int", func: bi_int }),
            ("range", NativeFn { name: "range", func: bi_range }),
            ("float", NativeFn { name: "float", func: bi_float }),
            ("bool", NativeFn { name: "bool", func: bi_bool }),
            ("pick", NativeFn { name: "pick", func: bi_pick }),
            ("shuffle", NativeFn { name: "shuffle", func: bi_shuffle }),
            ("seed", NativeFn { name: "seed", func: bi_seed }),
        ],
    )
}

thread_local! {
    static STATE: Cell<u64> = Cell::new(seed_from_time());
}

fn seed_from_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xDEADBEEF_CAFEBABE)
        | 1 // ensure non-zero
}

fn next_u64() -> u64 {
    STATE.with(|s| {
        let mut x = s.get();
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

fn bi_int(args: &[Value]) -> Result<Value, InterpreterError> {
    let max = match args.first() {
        Some(Value::Int(n)) => n.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "random.int".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "random.int".into(), span: None,
        }),
    };
    if max <= num_bigint::BigInt::from(0) {
        return Err(InterpreterError::RuntimeError {
            msg: "random.int: max must be positive".into(), span: None,
        });
    }
    let r = next_u64();
    Ok(Value::Int(num_bigint::BigInt::from(r) % max))
}

fn bi_range(args: &[Value]) -> Result<Value, InterpreterError> {
    let min = match args.get(0) {
        Some(Value::Int(n)) => n.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "random.range".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: 0, callee: "random.range".into(), span: None,
        }),
    };
    let max = match args.get(1) {
        Some(Value::Int(n)) => n.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "random.range".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "random.range".into(), span: None,
        }),
    };
    if max <= min {
        return Err(InterpreterError::RuntimeError {
            msg: "random.range: max must be greater than min".into(), span: None,
        });
    }
    let range = max - min.clone();
    let r = next_u64();
    Ok(Value::Int(min + (num_bigint::BigInt::from(r) % range)))
}

fn bi_float(_args: &[Value]) -> Result<Value, InterpreterError> {
    let r = next_u64();
    let f = (r as f64) / (u64::MAX as f64);
    Ok(Value::Decimal(BigDecimal::from_f64(f).unwrap_or_else(|| BigDecimal::from(0))))
}

fn bi_bool(_args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(next_u64() % 2 == 0))
}

fn bi_pick(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = match args.first() {
        Some(Value::Vec(v)) => v,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Vec", got: v.type_name(), op: "random.pick".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "random.pick".into(), span: None,
        }),
    };
    if v.is_empty() {
        return Err(InterpreterError::RuntimeError {
            msg: "random.pick: empty vec".into(), span: None,
        });
    }
    let idx = (next_u64() as usize) % v.len();
    Ok(v[idx].clone())
}

fn bi_shuffle(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = match args.first() {
        Some(Value::Vec(v)) => v,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Vec", got: v.type_name(), op: "random.shuffle".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "random.shuffle".into(), span: None,
        }),
    };
    let mut items: Vec<Value> = v.iter().cloned().collect();
    // Fisher-Yates shuffle
    for i in (1..items.len()).rev() {
        let j = (next_u64() as usize) % (i + 1);
        items.swap(i, j);
    }
    Ok(Value::vec(items))
}

fn bi_seed(args: &[Value]) -> Result<Value, InterpreterError> {
    let n = match args.first() {
        Some(Value::Int(n)) => n.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "random.seed".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 1, got: 0, callee: "random.seed".into(), span: None,
        }),
    };
    let s = num_traits::ToPrimitive::to_u64(&n).unwrap_or(1) | 1;
    STATE.with(|cell| cell.set(s));
    Ok(Value::Nil)
}
