//! Built-in functions registered into the global environment.
//!
//! Each builtin is a plain `fn(&[Value]) -> Result<Value, InterpreterError>`.
//! They are registered under their name (e.g. `"println"`, `"count"`, `"pow"`).

use crate::interpreter::env::EnvRef;
use crate::interpreter::error::InterpreterError;
use crate::interpreter::ops;
use crate::value::{NativeFn, Value};
use bigdecimal::BigDecimal;
use num_traits::{FromPrimitive, Signed, ToPrimitive};

/// Register all Phase-1 builtins into `env`.
pub fn register(env: &EnvRef) {
    let entries: &[(&'static str, NativeFn)] = &[
        // --- I/O ---
        ("println", NativeFn { name: "println", func: bi_println }),
        ("print", NativeFn { name: "print", func: bi_print }),
        // --- arithmetic ---
        ("pow", NativeFn { name: "pow", func: bi_pow }),
        ("abs", NativeFn { name: "abs", func: bi_abs }),
        // --- collection ops ---
        ("count", NativeFn { name: "count", func: bi_count }),
        ("first", NativeFn { name: "first", func: bi_first }),
        ("rest", NativeFn { name: "rest", func: bi_rest }),
        ("cons", NativeFn { name: "cons", func: bi_cons }),
        ("push", NativeFn { name: "push", func: bi_push }),
        ("assoc", NativeFn { name: "assoc", func: bi_assoc }),
        ("dissoc", NativeFn { name: "dissoc", func: bi_dissoc }),
        ("get", NativeFn { name: "get", func: bi_get }),
        ("has_key", NativeFn { name: "has_key", func: bi_has_key }),
        ("iter_to_vec", NativeFn { name: "iter_to_vec", func: bi_iter_to_vec }),
        // --- type predicates ---
        ("is_int", NativeFn { name: "is_int", func: bi_is_int }),
        ("is_decimal", NativeFn { name: "is_decimal", func: bi_is_decimal }),
        ("is_str", NativeFn { name: "is_str", func: bi_is_str }),
        ("is_bool", NativeFn { name: "is_bool", func: bi_is_bool }),
        ("is_nil", NativeFn { name: "is_nil", func: bi_is_nil }),
        ("is_vec", NativeFn { name: "is_vec", func: bi_is_vec }),
        ("is_map", NativeFn { name: "is_map", func: bi_is_map }),
        ("is_set", NativeFn { name: "is_set", func: bi_is_set }),
        ("is_number", NativeFn { name: "is_number", func: bi_is_number }),
        ("is_func", NativeFn { name: "is_func", func: bi_is_func }),
        ("is_closure", NativeFn { name: "is_closure", func: bi_is_closure }),
        ("type_of", NativeFn { name: "type_of", func: bi_type_of }),
        // --- introspection ---
        ("keys", NativeFn { name: "keys", func: bi_keys }),
        ("values", NativeFn { name: "values", func: bi_values }),
        ("fields", NativeFn { name: "fields", func: bi_fields }),
        ("variant_name", NativeFn { name: "variant_name", func: bi_variant_name }),
        ("variant_args", NativeFn { name: "variant_args", func: bi_variant_args }),
        ("instance_of", NativeFn { name: "instance_of", func: bi_instance_of }),
        ("ast_of", NativeFn { name: "ast_of", func: bi_ast_of }),
        // `eval` is a stub — the tree-walker and VM intercept calls by name
        // and dispatch to Interpreter::eval_src / Vm::eval_src so the call
        // has access to interpreter state (global env, etc.). The stub is
        // never invoked directly.
        ("eval", NativeFn { name: "eval", func: bi_eval_stub }),
        // --- conversions ---
        ("to_i64", NativeFn { name: "to_i64", func: bi_to_i64 }),
        ("to_f64", NativeFn { name: "to_f64", func: bi_to_f64 }),
        ("int", NativeFn { name: "int", func: bi_int }),
        ("decimal", NativeFn { name: "decimal", func: bi_decimal }),
        ("str", NativeFn { name: "str", func: bi_str }),
        ("to_str", NativeFn { name: "to_str", func: bi_to_str }),
        // --- higher-order (routed in call_function, not via func) ---
        ("map", NativeFn { name: "map", func: bi_higher_order_placeholder }),
        ("filter", NativeFn { name: "filter", func: bi_higher_order_placeholder }),
        ("fold", NativeFn { name: "fold", func: bi_higher_order_placeholder }),
        ("reduce", NativeFn { name: "reduce", func: bi_higher_order_placeholder }),
        ("find", NativeFn { name: "find", func: bi_higher_order_placeholder }),
        ("each", NativeFn { name: "each", func: bi_higher_order_placeholder }),
        // --- string ops (Phase 3.5c) ---
        ("len", NativeFn { name: "len", func: bi_len }),
        ("split", NativeFn { name: "split", func: bi_split }),
        ("join", NativeFn { name: "join", func: bi_join }),
        ("replace", NativeFn { name: "replace", func: bi_replace }),
        ("trim", NativeFn { name: "trim", func: bi_trim }),
        ("contains", NativeFn { name: "contains", func: bi_contains }),
        ("substring", NativeFn { name: "substring", func: bi_substring }),
        // --- string ops (lexer/parser infrastructure) ---
        ("starts_with", NativeFn { name: "starts_with", func: bi_starts_with }),
        ("ends_with", NativeFn { name: "ends_with", func: bi_ends_with }),
        ("index_of", NativeFn { name: "index_of", func: bi_index_of }),
        ("char_at", NativeFn { name: "char_at", func: bi_char_at }),
        ("codepoint_at", NativeFn { name: "codepoint_at", func: bi_codepoint_at }),
        ("from_codepoint", NativeFn { name: "from_codepoint", func: bi_from_codepoint }),
        ("byte_at", NativeFn { name: "byte_at", func: bi_byte_at }),
        ("byte_len", NativeFn { name: "byte_len", func: bi_byte_len }),
        ("to_lower", NativeFn { name: "to_lower", func: bi_to_lower }),
        ("to_upper", NativeFn { name: "to_upper", func: bi_to_upper }),
        ("is_digit", NativeFn { name: "is_digit", func: bi_is_digit }),
        ("is_alpha", NativeFn { name: "is_alpha", func: bi_is_alpha }),
        ("is_space", NativeFn { name: "is_space", func: bi_is_space }),
        // --- math (Phase 3.5d) ---
        ("min", NativeFn { name: "min", func: bi_min }),
        ("max", NativeFn { name: "max", func: bi_max }),
        ("floor", NativeFn { name: "floor", func: bi_floor }),
        ("ceil", NativeFn { name: "ceil", func: bi_ceil }),
        ("round", NativeFn { name: "round", func: bi_round }),
        ("sqrt", NativeFn { name: "sqrt", func: bi_sqrt }),
        ("sin", NativeFn { name: "sin", func: bi_sin }),
        ("cos", NativeFn { name: "cos", func: bi_cos }),
        ("log", NativeFn { name: "log", func: bi_log }),
        ("exp", NativeFn { name: "exp", func: bi_exp }),
        // --- async (Phase 4.7: Task combinators) ---
        ("task_all", NativeFn { name: "task_all", func: bi_task_all }),
        ("task_any", NativeFn { name: "task_any", func: bi_task_any }),
        ("task_ready", NativeFn { name: "task_ready", func: bi_task_ready }),
        // --- concurrency (Phase C3: actor introspection) ---
        ("pid_of", NativeFn { name: "pid_of", func: bi_pid_of }),
    ];
    for (name, nf) in entries {
        env.borrow_mut().define(*name, Value::Native(std::rc::Rc::new(nf.clone())));
    }
}

/// Placeholder for higher-order builtins (`map`/`filter`/`fold`/`reduce`/
/// `find`/`each`). These need to call user closures, so they are routed in
/// `Interpreter::call_function` instead of being invoked through `func`.
/// Reaching here means the builtin was called without going through the
/// interpreter's call path (should not happen in normal execution).
fn bi_higher_order_placeholder(_args: &[Value]) -> Result<Value, InterpreterError> {
    Err(InterpreterError::RuntimeError {
        msg: "higher-order builtin not available in this context".into(),
        span: None,
    })
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

fn bi_println(args: &[Value]) -> Result<Value, InterpreterError> {
    match args.first() {
        Some(Value::Str(s)) => println!("{}", s),
        Some(v) => println!("{}", v),
        None => println!(),
    }
    Ok(Value::Nil)
}

fn bi_print(args: &[Value]) -> Result<Value, InterpreterError> {
    match args.first() {
        Some(Value::Str(s)) => print!("{}", s),
        Some(v) => print!("{}", v),
        None => {}
    }
    Ok(Value::Nil)
}

// ---------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------

fn bi_pow(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2,
            got: args.len(),
            callee: "pow".into(),
            span: None,
        });
    }
    ops::pow(&args[0], &args[1])
}

fn bi_abs(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 1 {
        return Err(InterpreterError::ArityError {
            expected: 1,
            got: args.len(),
            callee: "abs".into(),
            span: None,
        });
    }
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Decimal(d) => Ok(Value::Decimal(d.abs())),
        _ => Err(InterpreterError::TypeError {
            expected: "number",
            got: args[0].type_name(),
            op: "abs".into(),
            span: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// collection ops
// ---------------------------------------------------------------------------

fn bi_count(args: &[Value]) -> Result<Value, InterpreterError> {
    one_arg(args, "count").and_then(|v| ops::count(&v).map(|n| Value::int(n)))
}

fn bi_first(args: &[Value]) -> Result<Value, InterpreterError> {
    one_arg(args, "first").and_then(|v| ops::first(&v))
}

fn bi_rest(args: &[Value]) -> Result<Value, InterpreterError> {
    one_arg(args, "rest").and_then(|v| ops::rest(&v))
}

fn bi_cons(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "cons".into(), span: None,
        });
    }
    ops::cons(&args[0], &args[1])
}

fn bi_push(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "push".into(), span: None,
        });
    }
    ops::push(&args[0], &args[1])
}

fn bi_assoc(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 3 {
        return Err(InterpreterError::ArityError {
            expected: 3, got: args.len(), callee: "assoc".into(), span: None,
        });
    }
    ops::assoc(&args[0], &args[1], &args[2])
}

fn bi_dissoc(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "dissoc".into(), span: None,
        });
    }
    ops::dissoc(&args[0], &args[1])
}

fn bi_get(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "get".into(), span: None,
        });
    }
    ops::get(&args[0], &args[1])
}

/// `has_key(map, key)` — whether `map` contains `key`. Works on Map only.
fn bi_has_key(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "has_key".into(), span: None,
        });
    }
    match &args[0] {
        Value::Map(m) => Ok(Value::Bool(m.contains_key(&args[1]))),
        _ => Err(InterpreterError::TypeError {
            expected: "Map",
            got: args[0].type_name(),
            op: "has_key".into(),
            span: None,
        }),
    }
}

/// `iter_to_vec(iterable)` — materialize any iterable into a Vec.
/// Used by the VM's `for...in` lowering so it can index with `get`.
fn bi_iter_to_vec(args: &[Value]) -> Result<Value, InterpreterError> {
    one_arg(args, "iter_to_vec").and_then(|v| ops::iter_to_vec(&v))
}

// ---------------------------------------------------------------------------
// type predicates
// ---------------------------------------------------------------------------

fn bi_is_int(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_int")?, Value::Int(_))))
}
fn bi_is_decimal(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_decimal")?, Value::Decimal(_))))
}
fn bi_is_str(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_str")?, Value::Str(_))))
}
fn bi_is_bool(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_bool")?, Value::Bool(_))))
}
fn bi_is_nil(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_nil")?, Value::Nil)))
}
fn bi_is_vec(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_vec")?, Value::Vec(_))))
}
fn bi_is_map(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_map")?, Value::Map(_))))
}
fn bi_is_set(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(one_arg(args, "is_set")?, Value::Set(_))))
}
fn bi_is_number(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(one_arg(args, "is_number")?.is_number()))
}
fn bi_is_func(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(
        one_arg(args, "is_func")?,
        Value::Func(_) | Value::Native(_) | Value::Closure(_)
    )))
}

/// `is_closure(v)` — true if `v` is a user-defined closure (either a
/// tree-walker `Func` or a VM `Closure`). Native builtins return false.
fn bi_is_closure(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::Bool(matches!(
        one_arg(args, "is_closure")?,
        Value::Func(_) | Value::Closure(_)
    )))
}

fn bi_type_of(args: &[Value]) -> Result<Value, InterpreterError> {
    Ok(Value::str(one_arg(args, "type_of")?.type_name()))
}

// ---------------------------------------------------------------------------
// Introspection: keys / values / fields / variant_{name,args} / instance_of
// ---------------------------------------------------------------------------

/// `keys(map)` → Vec of keys. Map keys are returned in iteration order.
/// Struct fields are returned as a Vec of field-name strings.
/// Returns an error for non-collection types.
fn bi_keys(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "keys")?;
    match v {
        Value::Map(m) => Ok(Value::vec(m.keys().cloned().collect())),
        Value::Struct { fields, .. } => Ok(Value::vec(
            fields.keys().map(|k| Value::str(k.clone())).collect(),
        )),
        _ => Err(InterpreterError::TypeError {
            expected: "Map or Struct",
            got: v.type_name(),
            op: "keys".into(),
            span: None,
        }),
    }
}

/// `values(map)` → Vec of values in iteration order. Struct field values
/// are returned in the same order as `keys`.
fn bi_values(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "values")?;
    match v {
        Value::Map(m) => Ok(Value::vec(m.values().cloned().collect())),
        Value::Struct { fields, .. } => Ok(Value::vec(fields.values().cloned().collect())),
        _ => Err(InterpreterError::TypeError {
            expected: "Map or Struct",
            got: v.type_name(),
            op: "values".into(),
            span: None,
        }),
    }
}

/// `fields(struct)` → Map of {field_name: value}. Errors for non-structs.
fn bi_fields(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "fields")?;
    match v {
        Value::Struct { fields, .. } => {
            let entries: Vec<(Value, Value)> = fields
                .iter()
                .map(|(k, val)| (Value::str(k.clone()), val.clone()))
                .collect();
            Ok(Value::map(entries))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "Struct",
            got: v.type_name(),
            op: "fields".into(),
            span: None,
        }),
    }
}

/// `variant_name(v)` → Str. The variant's name (e.g. "Some" for `Some(42)`).
fn bi_variant_name(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "variant_name")?;
    match v {
        Value::Variant { name, .. } => Ok(Value::str((*name).clone())),
        Value::Struct { name, .. } => Ok(Value::str((*name).clone())),
        _ => Err(InterpreterError::TypeError {
            expected: "Variant or Struct",
            got: v.type_name(),
            op: "variant_name".into(),
            span: None,
        }),
    }
}

/// `variant_args(v)` → Vec. The variant's arguments (empty for nullary).
fn bi_variant_args(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "variant_args")?;
    match v {
        Value::Variant { args, .. } => Ok(Value::vec(args.iter().cloned().collect())),
        Value::Struct { fields, .. } => Ok(Value::vec(fields.values().cloned().collect())),
        _ => Err(InterpreterError::TypeError {
            expected: "Variant or Struct",
            got: v.type_name(),
            op: "variant_args".into(),
            span: None,
        }),
    }
}

/// `instance_of(v, type_name)` → Bool.
///
/// - For `Variant { name, .. }` and `Struct { name, .. }`, true if
///   `type_name` matches the constructor name.
/// - Otherwise falls back to comparing `type_of(v)` with `type_name`
///   (so `instance_of(42, "Int")` is true).
fn bi_instance_of(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2,
            got: args.len(),
            callee: "instance_of".into(),
            span: None,
        });
    }
    let v = &args[0];
    let type_name = match &args[1] {
        Value::Str(s) => (**s).clone(),
        _ => {
            return Err(InterpreterError::TypeError {
                expected: "Str",
                got: args[1].type_name(),
                op: "instance_of".into(),
                span: None,
            })
        }
    };
    let matches = match v {
        Value::Variant { name, .. } | Value::Struct { name, .. } => {
            name.as_str() == type_name.as_str()
        }
        _ => {
            // Normalize string type names: both "Str" and "String" are
            // accepted (the language uses "Str" in predicates like
            // `is_str` but `type_of` returns "String"). Likewise "Func"
            // and "Closure" are interchangeable for closures.
            fn norm(s: &str) -> &str {
                match s {
                    "Str" => "String",
                    "Func" => "Closure",
                    other => other,
                }
            }
            norm(v.type_name()) == norm(&type_name)
        }
    };
    Ok(Value::Bool(matches))
}

/// `ast_of(src)` → Map. Parses `src` as 1y source and returns the AST
/// encoded as a 1y Map (see `ast::to_value`). On parse error, returns
/// an error Map `{"type": "ParseError", "message": str, "line": int, "col": int}`.
fn bi_ast_of(args: &[Value]) -> Result<Value, InterpreterError> {
    let src = match one_arg(args, "ast_of")? {
        Value::Str(s) => (*s).clone(),
        other => {
            return Err(InterpreterError::TypeError {
                expected: "Str",
                got: other.type_name(),
                op: "ast_of".into(),
                span: None,
            })
        }
    };
    let output = crate::parser::parse(&src);
    if !output.errors.is_empty() {
        let e = &output.errors[0];
        let msg = e.full_message();
        // Encode the parse error as a structured value so callers can
        // distinguish "parse failed" from "ast returned" without try/catch.
        let entries = vec![
            (Value::str("type".to_string()), Value::str("ParseError".to_string())),
            (Value::str("message".to_string()), Value::str(msg)),
            (
                Value::str("line".to_string()),
                Value::Int(e.span.start.line.into()),
            ),
            (
                Value::str("col".to_string()),
                Value::Int(e.span.start.col.into()),
            ),
        ];
        return Ok(Value::map(entries));
    }
    Ok(crate::ast::to_value::program_to_value(&output.program))
}

/// `eval(src)` stub. The real implementation lives in
/// `Interpreter::eval_src` / `Vm::eval_src` and is dispatched by name from
/// the native-call path. This function is never called directly — if it
/// ever is, it means the interpreter didn't intercept `eval` (a bug).
fn bi_eval_stub(_args: &[Value]) -> Result<Value, InterpreterError> {
    Err(InterpreterError::RuntimeError {
        msg: "eval is not available in this context (stub called)".into(),
        span: None,
    })
}

// ---------------------------------------------------------------------------
// conversions
// ---------------------------------------------------------------------------

fn bi_to_i64(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "to_i64")?;
    match &v {
        Value::Int(n) => Ok(Value::Int(n.clone())),
        Value::Decimal(d) => match d.to_i64() {
            Some(i) => Ok(Value::int(i)),
            None => Err(InterpreterError::RuntimeError {
                msg: format!("cannot convert {} to i64", d),
                span: None,
            }),
        },
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "to_i64".into(), span: None,
        }),
    }
}

fn bi_to_f64(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "to_f64")?;
    match &v {
        Value::Int(n) => match n.to_f64() {
            Some(f) => Ok(Value::Decimal(
                BigDecimal::from_f64(f).unwrap_or_else(|| BigDecimal::from(0)),
            )),
            None => Err(InterpreterError::RuntimeError {
                msg: format!("cannot convert {} to f64", n),
                span: None,
            }),
        },
        Value::Decimal(_) => Ok(v),
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "to_f64".into(), span: None,
        }),
    }
}

fn bi_int(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "int")?;
    match &v {
        Value::Int(_) => Ok(v),
        Value::Decimal(d) => {
            // Truncate towards zero by taking the integer part of the string repr.
            let s = format!("{}", d);
            let int_part = s.split('.').next().unwrap_or("0");
            match int_part.parse::<num_bigint::BigInt>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => Err(InterpreterError::RuntimeError {
                    msg: format!("cannot convert {} to int", d),
                    span: None,
                }),
            }
        }
        Value::Str(s) => match s.parse::<num_bigint::BigInt>() {
            Ok(n) => Ok(Value::Int(n)),
            Err(_) => Err(InterpreterError::RuntimeError {
                msg: format!("cannot convert \"{}\" to int", s),
                span: None,
            }),
        },
        _ => Err(InterpreterError::TypeError {
            expected: "number or Str", got: v.type_name(), op: "int".into(), span: None,
        }),
    }
}

fn bi_decimal(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "decimal")?;
    match &v {
        Value::Decimal(_) => Ok(v),
        Value::Int(n) => Ok(Value::Decimal(bigdecimal::BigDecimal::from(n.clone()))),
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "decimal".into(), span: None,
        }),
    }
}

fn bi_str(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "str")?;
    Ok(Value::str(format!("{}", v)))
}

/// `to_str(v)` — convert a value to its string representation for
/// interpolation. Unlike `str`, `Value::Str` yields its raw content (no
/// surrounding quotes), matching the tree-walker's string interpolation
/// semantics.
fn bi_to_str(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "to_str")?;
    match &v {
        Value::Str(s) => Ok(Value::str((**s).clone())),
        _ => Ok(Value::str(format!("{}", v))),
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn one_arg(args: &[Value], name: &str) -> Result<Value, InterpreterError> {
    args.first().cloned().ok_or_else(|| InterpreterError::ArityError {
        expected: 1,
        got: args.len(),
        callee: name.into(),
        span: None,
    })
}

// ---------------------------------------------------------------------------
// String operations (Phase 3.5c)
// ---------------------------------------------------------------------------

/// `len(x)` — size of a collection or string.
/// - Str: character count (not byte count)
/// - Vec/Set/Map: element count (same as `count`)
fn bi_len(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "len")?;
    match &v {
        Value::Str(s) => Ok(Value::int(s.chars().count() as i64)),
        _ => ops::count(&v).map(|n| Value::int(n)),
    }
}

/// `split(str, sep)` — split `str` by `sep`, returning a Vec of Str parts.
fn bi_split(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "split".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "split")?;
    let sep = str_arg(&args[1], "split")?;
    let parts: Vec<Value> = if sep.is_empty() {
        // Split into individual characters.
        s.chars().map(|c| Value::str(c.to_string())).collect()
    } else {
        s.split(sep).map(|p| Value::str(p.to_string())).collect()
    };
    Ok(Value::vec(parts))
}

/// `join(vec, sep)` — concatenate elements of `vec` (stringified) with `sep`.
/// Str elements use their raw content (no surrounding quotes).
fn bi_join(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "join".into(), span: None,
        });
    }
    let items = match &args[0] {
        Value::Vec(v) => v.iter().collect::<Vec<_>>(),
        _ => return Err(InterpreterError::TypeError {
            expected: "Vec", got: args[0].type_name(), op: "join".into(), span: None,
        }),
    };
    let sep = str_arg(&args[1], "join")?;
    let strings: Vec<String> = items
        .iter()
        .map(|v| match v {
            Value::Str(s) => (**s).clone(),
            _ => format!("{}", v),
        })
        .collect();
    Ok(Value::str(strings.join(sep)))
}

/// `replace(str, from, to)` — replace all occurrences of `from` with `to`.
fn bi_replace(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 3 {
        return Err(InterpreterError::ArityError {
            expected: 3, got: args.len(), callee: "replace".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "replace")?;
    let from = str_arg(&args[1], "replace")?;
    let to = str_arg(&args[2], "replace")?;
    Ok(Value::str(s.replace(from, to)))
}

/// `trim(str)` — strip leading and trailing whitespace.
fn bi_trim(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "trim")?;
    let s = str_arg(&v, "trim")?;
    Ok(Value::str(s.trim().to_string()))
}

/// `contains(str, substr)` — whether `str` contains `substr` as a substring.
fn bi_contains(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "contains".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "contains")?;
    let sub = str_arg(&args[1], "contains")?;
    Ok(Value::Bool(s.contains(sub)))
}

/// `substring(str, start, end)` — substring from `start` (inclusive) to `end`
/// (exclusive), using character indices. If `end` exceeds the string length,
/// the result goes to the end of the string.
fn bi_substring(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 3 {
        return Err(InterpreterError::ArityError {
            expected: 3, got: args.len(), callee: "substring".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "substring")?;
    let start = int_arg(&args[1], "substring", "start")?;
    let end = int_arg(&args[2], "substring", "end")?;
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    // Clamp indices to [0, n].
    let start = start.max(0).min(n) as usize;
    let end = end.max(start as i64).min(n) as usize;
    let result: String = chars[start..end].iter().collect();
    Ok(Value::str(result))
}

// --- string ops (lexer/parser infrastructure) ---
//
// These primitives are intentionally minimal so a 1y-written lexer/parser can
// compose them into higher-level scanners without needing regex or a native
// parser library.

/// `starts_with(s, prefix)` — whether `s` begins with `prefix`.
fn bi_starts_with(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "starts_with".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "starts_with")?;
    let p = str_arg(&args[1], "starts_with")?;
    Ok(Value::Bool(s.starts_with(p)))
}

/// `ends_with(s, suffix)` — whether `s` ends with `suffix`.
fn bi_ends_with(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "ends_with".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "ends_with")?;
    let p = str_arg(&args[1], "ends_with")?;
    Ok(Value::Bool(s.ends_with(p)))
}

/// `index_of(s, sub)` — character index of the first occurrence of `sub` in
/// `s`, or -1 if not found.
fn bi_index_of(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "index_of".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "index_of")?;
    let sub = str_arg(&args[1], "index_of")?;
    if sub.is_empty() {
        return Ok(Value::int(0));
    }
    // Find by byte position, then translate to char index.
    match s.find(sub) {
        Some(byte_pos) => {
            let char_pos = s[..byte_pos].chars().count() as i64;
            Ok(Value::int(char_pos))
        }
        None => Ok(Value::int(-1)),
    }
}

/// `char_at(s, i)` — the character at character-index `i` as a single-char
/// Str. Errors if out of range.
fn bi_char_at(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "char_at".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "char_at")?;
    let i = int_arg(&args[1], "char_at", "i")?;
    if i < 0 {
        return Err(InterpreterError::RuntimeError {
            msg: format!("char_at: index {} is negative", i), span: None,
        });
    }
    match s.chars().nth(i as usize) {
        Some(c) => Ok(Value::str(c.to_string())),
        None => Err(InterpreterError::RuntimeError {
            msg: format!("char_at: index {} out of range (len={})", i, s.chars().count()),
            span: None,
        }),
    }
}

/// `codepoint_at(s, i)` — Unicode codepoint of the char at character-index
/// `i`. Errors if out of range.
fn bi_codepoint_at(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "codepoint_at".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "codepoint_at")?;
    let i = int_arg(&args[1], "codepoint_at", "i")?;
    if i < 0 {
        return Err(InterpreterError::RuntimeError {
            msg: format!("codepoint_at: index {} is negative", i), span: None,
        });
    }
    match s.chars().nth(i as usize) {
        Some(c) => Ok(Value::int(c as u32 as i64)),
        None => Err(InterpreterError::RuntimeError {
            msg: format!("codepoint_at: index {} out of range (len={})", i, s.chars().count()),
            span: None,
        }),
    }
}

/// `from_codepoint(n)` — single-char string from a Unicode codepoint.
/// Errors if the codepoint is invalid (outside 0..=0x10FFFF or a surrogate).
fn bi_from_codepoint(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 1 {
        return Err(InterpreterError::ArityError {
            expected: 1, got: args.len(), callee: "from_codepoint".into(), span: None,
        });
    }
    let n = int_arg(&args[0], "from_codepoint", "n")?;
    if !(0..=0x10FFFF).contains(&n) {
        return Err(InterpreterError::RuntimeError {
            msg: format!("from_codepoint: {} out of Unicode range", n), span: None,
        });
    }
    match char::from_u32(n as u32) {
        Some(c) => Ok(Value::str(c.to_string())),
        None => Err(InterpreterError::RuntimeError {
            msg: format!("from_codepoint: {} is not a valid scalar value", n), span: None,
        }),
    }
}

/// `byte_at(s, i)` — byte value (0..255) at byte-index `i`. Errors if out
/// of range.
fn bi_byte_at(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "byte_at".into(), span: None,
        });
    }
    let s = str_arg(&args[0], "byte_at")?;
    let i = int_arg(&args[1], "byte_at", "i")?;
    if i < 0 {
        return Err(InterpreterError::RuntimeError {
            msg: format!("byte_at: index {} is negative", i), span: None,
        });
    }
    match s.as_bytes().get(i as usize) {
        Some(&b) => Ok(Value::int(b as i64)),
        None => Err(InterpreterError::RuntimeError {
            msg: format!("byte_at: index {} out of range (byte_len={})", i, s.len()),
            span: None,
        }),
    }
}

/// `byte_len(s)` — number of UTF-8 bytes in `s`.
fn bi_byte_len(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "byte_len")?;
    let s = str_arg(&v, "byte_len")?;
    Ok(Value::int(s.len() as i64))
}

/// `to_lower(s)` — lowercase copy of `s`.
fn bi_to_lower(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "to_lower")?;
    let s = str_arg(&v, "to_lower")?;
    Ok(Value::str(s.to_lowercase()))
}

/// `to_upper(s)` — uppercase copy of `s`.
fn bi_to_upper(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "to_upper")?;
    let s = str_arg(&v, "to_upper")?;
    Ok(Value::str(s.to_uppercase()))
}

/// `is_digit(s)` — whether `s` is a single ASCII digit '0'..'9'.
fn bi_is_digit(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "is_digit")?;
    let s = str_arg(&v, "is_digit")?;
    Ok(Value::Bool(s.len() == 1 && s.as_bytes()[0].is_ascii_digit()))
}

/// `is_alpha(s)` — whether `s` is a single ASCII alphabetic char [A-Za-z].
fn bi_is_alpha(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "is_alpha")?;
    let s = str_arg(&v, "is_alpha")?;
    Ok(Value::Bool(s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic()))
}

/// `is_space(s)` — whether `s` is a single ASCII whitespace char
/// (' ', '\t', '\n', '\r').
fn bi_is_space(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "is_space")?;
    let s = str_arg(&v, "is_space")?;
    Ok(Value::Bool(matches!(s, " " | "\t" | "\n" | "\r")))
}

/// Extract a `&str` from a `Value::Str` argument.
fn str_arg<'a>(v: &'a Value, fn_name: &str) -> Result<&'a str, InterpreterError> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        _ => Err(InterpreterError::TypeError {
            expected: "Str",
            got: v.type_name(),
            op: fn_name.into(),
            span: None,
        }),
    }
}

/// Extract a non-negative `i64` from a `Value::Int` argument.
fn int_arg(v: &Value, fn_name: &str, param: &str) -> Result<i64, InterpreterError> {
    match v {
        Value::Int(n) => Ok(n.to_i64().unwrap_or(0)),
        _ => Err(InterpreterError::TypeError {
            expected: "Int",
            got: v.type_name(),
            op: format!("{}: {}", fn_name, param),
            span: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Math (Phase 3.5d)
// ---------------------------------------------------------------------------

/// Convert a numeric `Value` (Int or Decimal) to `f64`.
fn num_to_f64(v: &Value, fn_name: &str) -> Result<f64, InterpreterError> {
    match v {
        Value::Int(n) => n.to_f64().ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("cannot convert {} to f64", n),
            span: None,
        }),
        Value::Decimal(d) => d.to_f64().ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("cannot convert {} to f64", d),
            span: None,
        }),
        _ => Err(InterpreterError::TypeError {
            expected: "number",
            got: v.type_name(),
            op: fn_name.into(),
            span: None,
        }),
    }
}

/// Wrap an `f64` as a `Value::Decimal`, mapping NaN/Infinity to an error.
fn f64_to_decimal(f: f64, fn_name: &str) -> Result<Value, InterpreterError> {
    if f.is_nan() || f.is_infinite() {
        return Err(InterpreterError::RuntimeError {
            msg: format!("{} produced a non-finite result", fn_name),
            span: None,
        });
    }
    Ok(Value::Decimal(BigDecimal::from_f64(f).unwrap_or_else(|| BigDecimal::from(0))))
}

/// `min(a, b)` — smaller of two numbers (preserves original type).
fn bi_min(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "min".into(), span: None,
        });
    }
    let a = num_to_f64(&args[0], "min")?;
    let b = num_to_f64(&args[1], "min")?;
    Ok(if a <= b { args[0].clone() } else { args[1].clone() })
}

/// `max(a, b)` — larger of two numbers (preserves original type).
fn bi_max(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "max".into(), span: None,
        });
    }
    let a = num_to_f64(&args[0], "max")?;
    let b = num_to_f64(&args[1], "max")?;
    Ok(if a >= b { args[0].clone() } else { args[1].clone() })
}

/// `floor(n)` — largest integer ≤ n. Returns Int.
fn bi_floor(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "floor")?;
    match &v {
        Value::Int(_) => Ok(v),
        Value::Decimal(d) => {
            let f = d.to_f64().ok_or_else(|| InterpreterError::RuntimeError {
                msg: format!("cannot floor {}", d), span: None,
            })?;
            Ok(Value::int(f.floor() as i64))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "floor".into(), span: None,
        }),
    }
}

/// `ceil(n)` — smallest integer ≥ n. Returns Int.
fn bi_ceil(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "ceil")?;
    match &v {
        Value::Int(_) => Ok(v),
        Value::Decimal(d) => {
            let f = d.to_f64().ok_or_else(|| InterpreterError::RuntimeError {
                msg: format!("cannot ceil {}", d), span: None,
            })?;
            Ok(Value::int(f.ceil() as i64))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "ceil".into(), span: None,
        }),
    }
}

/// `round(n)` — nearest integer (half away from zero). Returns Int.
fn bi_round(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "round")?;
    match &v {
        Value::Int(_) => Ok(v),
        Value::Decimal(d) => {
            let f = d.to_f64().ok_or_else(|| InterpreterError::RuntimeError {
                msg: format!("cannot round {}", d), span: None,
            })?;
            Ok(Value::int(f.round() as i64))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "number", got: v.type_name(), op: "round".into(), span: None,
        }),
    }
}

/// `sqrt(n)` — square root. Returns Decimal.
fn bi_sqrt(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "sqrt")?;
    let f = num_to_f64(&v, "sqrt")?;
    f64_to_decimal(f.sqrt(), "sqrt")
}

/// `sin(n)` — sine (radians). Returns Decimal.
fn bi_sin(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "sin")?;
    let f = num_to_f64(&v, "sin")?;
    f64_to_decimal(f.sin(), "sin")
}

/// `cos(n)` — cosine (radians). Returns Decimal.
fn bi_cos(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "cos")?;
    let f = num_to_f64(&v, "cos")?;
    f64_to_decimal(f.cos(), "cos")
}

/// `log(n, base)` — logarithm of `n` to `base`. Returns Decimal.
fn bi_log(args: &[Value]) -> Result<Value, InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "log".into(), span: None,
        });
    }
    let n = num_to_f64(&args[0], "log")?;
    let base = num_to_f64(&args[1], "log")?;
    if n <= 0.0 || base <= 0.0 || base == 1.0 {
        return Err(InterpreterError::RuntimeError {
            msg: "log requires n > 0 and base > 0, base != 1".into(),
            span: None,
        });
    }
    f64_to_decimal(n.log(base), "log")
}

/// `exp(n)` — e^n. Returns Decimal.
fn bi_exp(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "exp")?;
    let f = num_to_f64(&v, "exp")?;
    f64_to_decimal(f.exp(), "exp")
}

// ---------------------------------------------------------------------------
// Task combinators (Phase 4.7: colorless async)
// ---------------------------------------------------------------------------

/// `task_all([t1, t2, ...]) -> Task<Vec<value>>`
///
/// Returns a Task that completes when ALL input tasks complete.
/// On each poll, checks every child task; if any is still Pending,
/// the combined task is Pending. When all are Ready, collects their
/// values into a Vec and completes.
///
/// Child tasks are consumed (marked Consumed) when they complete.
fn bi_task_all(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "task_all")?;
    let tasks: Vec<crate::value::TaskRef> = match &v {
        Value::Vec(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Task(t) => out.push(t.clone()),
                    other => return Err(InterpreterError::TypeError {
                        expected: "Task", got: other.type_name(),
                        op: format!("task_all[{}]", i), span: None,
                    }),
                }
            }
            out
        }
        other => return Err(InterpreterError::TypeError {
            expected: "Vec", got: other.type_name(), op: "task_all".into(), span: None,
        }),
    };

    let combined = crate::value::TaskState::Pending(Box::new(move || {
        let mut results = Vec::with_capacity(tasks.len());
        for t in &tasks {
            let val = {
                let task_ref = t.borrow();
                match &*task_ref {
                    crate::value::TaskState::Ready(v) => v.clone(),
                    crate::value::TaskState::Consumed => Value::Nil,
                    crate::value::TaskState::Pending(f, _) => match f() {
                        crate::value::TaskPoll::Ready(v) => v,
                        crate::value::TaskPoll::Pending => return crate::value::TaskPoll::Pending,
                    },
                }
            };
            // Mark consumed if it was Ready or just became Ready.
            {
                let mut task_ref = t.borrow_mut();
                if !matches!(*task_ref, crate::value::TaskState::Consumed) {
                    *task_ref = crate::value::TaskState::Consumed;
                }
            }
            results.push(val);
        }
        crate::value::TaskPoll::Ready(Value::vec(results))
    }), None);
    Ok(Value::Task(std::rc::Rc::new(std::cell::RefCell::new(combined))))
}

/// `task_any([t1, t2, ...]) -> Task<value>`
///
/// Returns a Task that completes when ANY input task completes.
/// On each poll, checks every child task; the first one that is Ready
/// (or becomes Ready during poll) completes the combined task with that
/// value. If all are still Pending, the combined task is Pending.
///
/// Only the winning child task is consumed; others remain untouched.
fn bi_task_any(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "task_any")?;
    let tasks: Vec<crate::value::TaskRef> = match &v {
        Value::Vec(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Task(t) => out.push(t.clone()),
                    other => return Err(InterpreterError::TypeError {
                        expected: "Task", got: other.type_name(),
                        op: format!("task_any[{}]", i), span: None,
                    }),
                }
            }
            out
        }
        other => return Err(InterpreterError::TypeError {
            expected: "Vec", got: other.type_name(), op: "task_any".into(), span: None,
        }),
    };

    let combined = crate::value::TaskState::Pending(Box::new(move || {
        for t in &tasks {
            let ready_val = {
                let task_ref = t.borrow();
                match &*task_ref {
                    crate::value::TaskState::Ready(v) => Some(v.clone()),
                    crate::value::TaskState::Consumed => None,
                    crate::value::TaskState::Pending(f, _) => match f() {
                        crate::value::TaskPoll::Ready(v) => Some(v),
                        crate::value::TaskPoll::Pending => None,
                    },
                }
            };
            if let Some(v) = ready_val {
                // Consume the winning task.
                {
                    let mut task_ref = t.borrow_mut();
                    *task_ref = crate::value::TaskState::Consumed;
                }
                return crate::value::TaskPoll::Ready(v);
            }
        }
        crate::value::TaskPoll::Pending
    }), None);
    Ok(Value::Task(std::rc::Rc::new(std::cell::RefCell::new(combined))))
}

/// `task_ready(value) -> Task`
///
/// Creates a Task that is immediately completed with `value`.
/// Useful for:
///   - wrapping a synchronous value into a Task to unify interfaces
///   - seeding `task_all` / `task_any` with a known value
///   - prototyping async code without real I/O
///
/// Example:
///   let t = task_ready(42);
///   let v = await t;       # v == 42, no suspension
fn bi_task_ready(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "task_ready")?;
    let task = crate::value::TaskState::Ready(v);
    Ok(Value::Task(std::rc::Rc::new(std::cell::RefCell::new(task))))
}

/// `pid_of(actor) -> Int`
///
/// Returns the globally-unique actor ID (Pid) of an actor instance as an
/// integer. Pids are allocated at spawn time and registered in the global
/// `ActorRegistry`, allowing actors on other threads to route messages
/// to this actor via its Pid.
///
/// Example:
///   let c = spawn Counter();
///   let id = pid_of(c);     # e.g. 42
fn bi_pid_of(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "pid_of")?;
    match &v {
        Value::Actor(ar) => {
            let pid = ar.borrow().pid;
            Ok(Value::Int(num_bigint::BigInt::from(pid.0)))
        }
        other => Err(InterpreterError::TypeError {
            expected: "Actor",
            got: other.type_name(),
            op: "pid_of".into(),
            span: None,
        }),
    }
}
