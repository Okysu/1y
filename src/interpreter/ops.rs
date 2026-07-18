//! Built-in operations on [`Value`]: arithmetic with auto-promotion,
//! comparison, and persistent-collection updates.
//!
//! # Numeric tower
//!
//! - `Int(BigInt)` + `Int(BigInt)` → `Int(BigInt)` (never overflows)
//! - Mixed `Int` / `Decimal` → `Decimal(BigDecimal)` (auto-promote)
//! - `/` (true division): `Int / Int` returns `Int` if evenly divisible,
//!   otherwise promotes to `Decimal` (never truncates)
//! - `%` (modulo): both operands must be `Int`

use crate::interpreter::error::InterpreterError;
use crate::value::Value;
use bigdecimal::BigDecimal;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

pub fn add(a: &Value, b: &Value) -> Result<Value, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        (Value::Decimal(x), Value::Decimal(y)) => Ok(Value::Decimal(x + y)),
        (Value::Str(x), Value::Str(y)) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(x);
            s.push_str(y);
            Ok(Value::str(s))
        }
        _ => {
            if let (Some(x), Some(y)) = (a.to_decimal(), b.to_decimal()) {
                Ok(Value::Decimal(x + y))
            } else {
                Err(type_err("arithmetic", "+", a, b))
            }
        }
    }
}

pub fn sub(a: &Value, b: &Value) -> Result<Value, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        (Value::Decimal(x), Value::Decimal(y)) => Ok(Value::Decimal(x - y)),
        _ => {
            if let (Some(x), Some(y)) = (a.to_decimal(), b.to_decimal()) {
                Ok(Value::Decimal(x - y))
            } else {
                Err(type_err("arithmetic", "-", a, b))
            }
        }
    }
}

pub fn mul(a: &Value, b: &Value) -> Result<Value, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
        (Value::Decimal(x), Value::Decimal(y)) => Ok(Value::Decimal(x * y)),
        _ => {
            if let (Some(x), Some(y)) = (a.to_decimal(), b.to_decimal()) {
                Ok(Value::Decimal(x * y))
            } else {
                Err(type_err("arithmetic", "*", a, b))
            }
        }
    }
}

/// True division: `a / b`. Never truncates — `7 / 2` → `Decimal(3.5)`.
/// If both are Int and evenly divisible, returns Int.
pub fn div(a: &Value, b: &Value) -> Result<Value, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if y.is_zero() {
                Err(InterpreterError::DivisionByZero { span: None })
            } else if x % y == BigInt::zero() {
                Ok(Value::Int(x / y))
            } else {
                // Promote to decimal for true division.
                let dx = BigDecimal::from(x.clone());
                let dy = BigDecimal::from(y.clone());
                Ok(Value::Decimal(dx / dy))
            }
        }
        (Value::Decimal(x), Value::Decimal(y)) => {
            if y.is_zero() {
                Err(InterpreterError::DivisionByZero { span: None })
            } else {
                Ok(Value::Decimal(x / y))
            }
        }
        _ => {
            if let (Some(x), Some(y)) = (a.to_decimal(), b.to_decimal()) {
                if y.is_zero() {
                    Err(InterpreterError::DivisionByZero { span: None })
                } else {
                    Ok(Value::Decimal(x / y))
                }
            } else {
                Err(type_err("arithmetic", "/", a, b))
            }
        }
    }
}

pub fn modulo(a: &Value, b: &Value) -> Result<Value, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if y.is_zero() {
                Err(InterpreterError::DivisionByZero { span: None })
            } else {
                Ok(Value::Int(x % y))
            }
        }
        _ => Err(type_err("modulo", "%", a, b)),
    }
}

/// `pow(base, exp)`: Int^non-negative-Int → Int. Any Decimal involved → Decimal.
pub fn pow(base: &Value, exp: &Value) -> Result<Value, InterpreterError> {
    match (base, exp) {
        (Value::Int(b), Value::Int(e)) => {
            if e.is_negative() {
                // Negative exponent: promote to decimal.
                let db = BigDecimal::from(b.clone());
                let de = BigDecimal::from(e.clone());
                Ok(Value::Decimal(decimal_pow(&db, &de)?))
            } else {
                let eu = e.to_u32().ok_or_else(|| {
                    InterpreterError::RuntimeError {
                        msg: format!("exponent too large: {}", e),
                        span: None,
                    }
                })?;
                Ok(Value::Int(b.pow(eu)))
            }
        }
        _ => {
            if let (Some(b), Some(e)) = (base.to_decimal(), exp.to_decimal()) {
                Ok(Value::Decimal(decimal_pow(&b, &e)?))
            } else {
                Err(type_err("power", "pow", base, exp))
            }
        }
    }
}

fn decimal_pow(base: &BigDecimal, exp: &BigDecimal) -> Result<BigDecimal, InterpreterError> {
    // BigDecimal doesn't have a native pow; use f64 approximation for now.
    // This is a known limitation — exact decimal exponentiation is complex
    // (requires exp/ln). For integer exponents we can do repeated multiply.
    let exp_int = exp.to_i64();
    match exp_int {
        Some(e) if e >= 0 => {
            let mut result = BigDecimal::from(1);
            for _ in 0..e {
                result *= base;
            }
            Ok(result)
        }
        Some(e) => {
            // Negative integer exponent: 1 / base^|e|
            let mut result = BigDecimal::from(1);
            for _ in 0..(-e) {
                result *= base;
            }
            if result.is_zero() {
                Err(InterpreterError::DivisionByZero { span: None })
            } else {
                Ok(BigDecimal::from(1) / result)
            }
        }
        None => Err(InterpreterError::RuntimeError {
            msg: format!("non-integer decimal exponent: {}", exp),
            span: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

pub fn eq(a: &Value, b: &Value) -> bool {
    a == b
}

pub fn lt(a: &Value, b: &Value) -> Result<bool, InterpreterError> {
    cmp(a, b, "<").map(|o| o == std::cmp::Ordering::Less)
}

pub fn gt(a: &Value, b: &Value) -> Result<bool, InterpreterError> {
    cmp(a, b, ">").map(|o| o == std::cmp::Ordering::Greater)
}

pub fn lte(a: &Value, b: &Value) -> Result<bool, InterpreterError> {
    cmp(a, b, "<=").map(|o| o != std::cmp::Ordering::Greater)
}

pub fn gte(a: &Value, b: &Value) -> Result<bool, InterpreterError> {
    cmp(a, b, ">=").map(|o| o != std::cmp::Ordering::Less)
}

fn cmp(a: &Value, b: &Value, op: &str) -> Result<std::cmp::Ordering, InterpreterError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Decimal(x), Value::Decimal(y)) => Ok(x.cmp(y)),
        _ => {
            if let (Some(x), Some(y)) = (a.to_decimal(), b.to_decimal()) {
                Ok(x.cmp(&y))
            } else {
                Err(type_err("comparison", op, a, b))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Collection operations
// ---------------------------------------------------------------------------

/// `count(xs)` → number of elements.
pub fn count(v: &Value) -> Result<usize, InterpreterError> {
    match v {
        Value::Vec(v) => Ok(v.len()),
        Value::Map(m) => Ok(m.len()),
        Value::Set(s) => Ok(s.len()),
        Value::Str(s) => Ok(s.chars().count()),
        _ => Err(InterpreterError::TypeError {
            expected: "collection",
            got: v.type_name(),
            op: "count".into(),
            span: None,
        }),
    }
}

/// `first(xs)` → first element (or nil if empty).
pub fn first(v: &Value) -> Result<Value, InterpreterError> {
    match v {
        Value::Vec(v) => Ok(v.front().cloned().unwrap_or(Value::Nil)),
        _ => Err(InterpreterError::TypeError {
            expected: "Vec",
            got: v.type_name(),
            op: "first".into(),
            span: None,
        }),
    }
}

/// `rest(xs)` → all elements except the first (as the same collection type).
pub fn rest(v: &Value) -> Result<Value, InterpreterError> {
    match v {
        Value::Vec(v) => {
            if v.is_empty() {
                Ok(Value::Vec(v.clone()))
            } else {
                Ok(Value::Vec(v.iter().skip(1).cloned().collect()))
            }
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Vec",
            got: v.type_name(),
            op: "rest".into(),
            span: None,
        }),
    }
}

/// `cons(x, xs)` → prepend `x` to `xs`. Works on Vec.
pub fn cons(head: &Value, tail: &Value) -> Result<Value, InterpreterError> {
    match tail {
        Value::Vec(v) => {
            let mut new = v.clone();
            new.push_front(head.clone());
            Ok(Value::Vec(new))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Vec",
            got: tail.type_name(),
            op: "cons".into(),
            span: None,
        }),
    }
}

/// `push(xs, x)` → append `x` to `xs` (returns a new Vec).
pub fn push(coll: &Value, item: &Value) -> Result<Value, InterpreterError> {
    match coll {
        Value::Vec(v) => {
            let mut new = v.clone();
            new.push_back(item.clone());
            Ok(Value::Vec(new))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Vec",
            got: coll.type_name(),
            op: "push".into(),
            span: None,
        }),
    }
}

/// `assoc(map, key, value)` → new map with `key` set to `value`.
pub fn assoc(map: &Value, key: &Value, value: &Value) -> Result<Value, InterpreterError> {
    match map {
        Value::Map(m) => Ok(Value::Map(m.update(key.clone(), value.clone()))),
        Value::Vec(v) => {
            // Vec index assignment: `v[i] = value`. `i` must be a non-negative
            // Int within bounds. (Out-of-bounds insertion is not supported;
            // use `push` for that.)
            let i = match key {
                Value::Int(n) => n.to_usize(),
                _ => return Err(InterpreterError::TypeError {
                    expected: "Int",
                    got: key.type_name(),
                    op: "assoc (vec index)".into(),
                    span: None,
                }),
            };
            match i {
                Some(i) if i < v.len() => {
                    let mut new_v = v.clone();
                    new_v[i] = value.clone();
                    Ok(Value::Vec(new_v))
                }
                _ => Err(InterpreterError::IndexError {
                    msg: format!("index out of bounds: {}", key),
                    span: None,
                }),
            }
        }
        // Struct field assignment: `obj.field = value`. Mirrors the
        // tree-walker — full clone of the fields HashMap, insert, rebuild
        // Value::Struct. Field types are not validated (dynamic typing).
        Value::Struct { name, fields } => {
            let field_name = match key {
                Value::Str(s) => (**s).clone(),
                _ => return Err(InterpreterError::TypeError {
                    expected: "Str",
                    got: key.type_name(),
                    op: "assoc (struct field)".into(),
                    span: None,
                }),
            };
            let mut new_fields = (**fields).clone();
            new_fields.insert(field_name, value.clone());
            Ok(Value::Struct {
                name: name.clone(),
                fields: Rc::new(new_fields),
            })
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Map, Vec, or Struct",
            got: map.type_name(),
            op: "assoc".into(),
            span: None,
        }),
    }
}

/// `dissoc(map, key)` → new map with `key` removed.
pub fn dissoc(map: &Value, key: &Value) -> Result<Value, InterpreterError> {
    match map {
        Value::Map(m) => Ok(Value::Map(m.without(key))),
        _ => Err(InterpreterError::TypeError {
            expected: "Map",
            got: map.type_name(),
            op: "dissoc".into(),
            span: None,
        }),
    }
}

/// `get(coll, key)` → value at `key`, or `nil` if absent.
pub fn get(coll: &Value, key: &Value) -> Result<Value, InterpreterError> {
    match coll {
        Value::Map(m) => Ok(m.get(key).cloned().unwrap_or(Value::Nil)),
        Value::Vec(v) => {
            let idx = match key {
                Value::Int(n) => n.to_usize(),
                _ => return Err(type_err("indexing", "get", key, coll)),
            };
            Ok(idx
                .and_then(|i| v.get(i))
                .cloned()
                .unwrap_or(Value::Nil))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Map or Vec",
            got: coll.type_name(),
            op: "get".into(),
            span: None,
        }),
    }
}

/// `iter_to_vec(v)` → Vec of items from any iterable (Vec/Set/Map/Str).
///
/// Mirrors the tree-walker's `Interpreter::iter_to_vec` so that the VM's
/// `for...in` (which compiles to `count` + `get` indexing) can iterate over
/// all the same types by first materializing them into a Vec:
///
/// - `Vec`  → itself (cloned)
/// - `Set`  → elements in iteration order
/// - `Map`  → `[key, value]` pairs
/// - `Str`  → single-character strings (Unicode chars)
pub fn iter_to_vec(v: &Value) -> Result<Value, InterpreterError> {
    let out = match v {
        Value::Vec(v) => v.iter().cloned().collect::<Vec<_>>(),
        Value::Set(s) => s.iter().cloned().collect::<Vec<_>>(),
        Value::Map(m) => m
            .iter()
            .map(|(k, val)| Value::vec(vec![k.clone(), val.clone()]))
            .collect::<Vec<_>>(),
        Value::Str(s) => s
            .chars()
            .map(|c| Value::str(c.to_string()))
            .collect::<Vec<_>>(),
        _ => {
            return Err(InterpreterError::TypeError {
                expected: "Vec, Set, Map, or Str (iterable)",
                got: v.type_name(),
                op: "iter_to_vec".into(),
                span: None,
            })
        }
    };
    Ok(Value::vec(out))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn type_err(category: &'static str, op: &str, a: &Value, b: &Value) -> InterpreterError {
    InterpreterError::TypeError {
        expected: category,
        got: a.type_name(),
        op: format!("{} ({})", op, b.type_name()),
        span: None,
    }
}
