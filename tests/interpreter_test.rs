//! Phase 1 interpreter tests: tree-walking evaluator.
//!
//! Covers arithmetic with auto-promotion, persistent collections, pattern
//! matching, closures, exceptions, pipe, and custom types.

use onely::Interpreter;
use onely::Value;

fn eval(src: &str) -> Value {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

fn eval_err(src: &str) -> String {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(_) => panic!("expected error, got success"),
        Err(e) => format!("{}", e),
    }
}

// Tree-walking interpreters map `1y` recursion to native Rust recursion, so
// deep recursion (e.g. `fact(100)`) overflows the default 1 MB test-thread
// stack on Windows. This helper runs an evaluation on a worker thread with a
// large stack and returns the resulting `Value`'s `Display` string (`Value`
// itself is `!Send` because it holds `Rc`).
fn eval_big(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let mut interp = Interpreter::new();
            match interp.eval_source(&src) {
                Ok(v) => format!("{}", v),
                Err(e) => panic!("eval failed: {}", e),
            }
        })
        .expect("spawn eval thread")
        .join()
        .expect("eval thread panicked")
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

#[test]
fn test_integer_arithmetic() {
    assert_eq!(eval("1 + 2 * 3"), eval("7"));
    assert_eq!(eval("(1 + 2) * 3"), eval("9"));
    assert_eq!(eval("10 - 3 - 2"), eval("5"));
    assert_eq!(eval("2 * 3 + 4 * 5"), eval("26"));
}

#[test]
fn test_true_division_promotes_to_decimal() {
    // 7 / 2 should be 3.5 (Decimal), not 3 (truncated Int)
    match eval("7 / 2") {
        Value::Decimal(_) => {}
        other => panic!("expected Decimal, got {:?}", other),
    }
    // 6 / 2 should stay Int (evenly divisible)
    match eval("6 / 2") {
        Value::Int(_) => {}
        other => panic!("expected Int, got {:?}", other),
    }
}

#[test]
fn test_mixed_arithmetic_promotes() {
    // Int + Decimal → Decimal
    match eval("1 + 0.5") {
        Value::Decimal(_) => {}
        other => panic!("expected Decimal, got {:?}", other),
    }
    // Decimal * Int → Decimal
    match eval("2.5 * 4") {
        Value::Decimal(_) => {}
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_modulo_int_only() {
    assert_eq!(eval("7 % 3"), eval("1"));
    assert_eq!(eval("10 % 4"), eval("2"));
}

#[test]
fn test_comparison() {
    assert_eq!(eval("1 < 2"), Value::Bool(true));
    assert_eq!(eval("3 >= 3"), Value::Bool(true));
    assert_eq!(eval("1.5 > 2.5"), Value::Bool(false));
    assert_eq!(eval("1 == 1"), Value::Bool(true));
    assert_eq!(eval("1 != 2"), Value::Bool(true));
}

#[test]
fn test_short_circuit_and_or() {
    assert_eq!(eval("true and false"), Value::Bool(false));
    assert_eq!(eval("true or false"), Value::Bool(true));
    assert_eq!(eval("false and 1/0 == 0"), Value::Bool(false)); // short-circuits, no div-by-zero
    assert_eq!(eval("true or 1/0 == 0"), Value::Bool(true));    // short-circuits
}

#[test]
fn test_unary_negation_and_not() {
    assert_eq!(eval("-5"), eval("-5"));
    assert_eq!(eval("not true"), Value::Bool(false));
    assert_eq!(eval("not false"), Value::Bool(true));
    assert_eq!(eval("not nil"), Value::Bool(true));
}

// ---------------------------------------------------------------------------
// Big integer: 100-digit factorial
// ---------------------------------------------------------------------------

#[test]
fn test_factorial_100() {
    let src = r#"
        fn fact(n) -> Int {
            if n <= 1 {
                1
            } else {
                n * fact(n - 1)
            }
        }
        fact(100)
    "#;
    // Deep recursion: run on a worker thread with a large stack.
    let s = eval_big(src);
    // 100! has 158 digits
    assert_eq!(s.len(), 158, "100! should have 158 digits, got {}: {}...", &s[..20], s.len());
    assert!(s.starts_with("93326215443944152681699238856266700490715968264381621468592963895217599993229915608941463976156518286253697920827223758251185210916864000000000000000000000000"));
}

#[test]
fn test_bigint_no_overflow() {
    // 2^100 should not overflow
    let src = "pow(2, 100)";
    let result = eval(src);
    match result {
        Value::Int(n) => {
            let s = n.to_string();
            assert_eq!(s, "1267650600228229401496703205376");
        }
        other => panic!("expected Int, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Persistent collections
// ---------------------------------------------------------------------------

#[test]
fn test_vector_literal_and_indexing() {
    assert_eq!(eval("[1, 2, 3][0]"), eval("1"));
    assert_eq!(eval("[1, 2, 3][2]"), eval("3"));
    assert_eq!(eval("count([1, 2, 3])"), eval("3"));
    assert_eq!(eval("first([1, 2, 3])"), eval("1"));
}

#[test]
fn test_vector_push_preserves_original() {
    let src = r#"
        let v = [1, 2, 3];
        let v2 = push(v, 4);
        count(v)
    "#;
    assert_eq!(eval(src), eval("3")); // v unchanged
}

#[test]
fn test_map_literal_and_access() {
    let src = r#"
        let m = { "a": 1, "b": 2 };
        get(m, "a")
    "#;
    assert_eq!(eval(src), eval("1"));
}

#[test]
fn test_map_assoc_preserves_original() {
    let src = r#"
        let m = { "x": 1 };
        let m2 = assoc(m, "y", 2);
        count(m)
    "#;
    assert_eq!(eval(src), eval("1")); // m unchanged
}

#[test]
fn test_set_literal() {
    assert_eq!(eval("count(#{1, 2, 3})"), eval("3"));
}

#[test]
fn test_cons_prepends() {
    assert_eq!(eval("first(cons(0, [1, 2, 3]))"), eval("0"));
    assert_eq!(eval("count(cons(0, [1, 2, 3]))"), eval("4"));
}

// ---------------------------------------------------------------------------
// Closures and functions
// ---------------------------------------------------------------------------

#[test]
fn test_recursive_function() {
    // Tree-walking interpreters map `1y` recursion to native Rust recursion.
    // `eval_expr` is a large `match` whose stack frame grew after Phase 3.5a
    // added loop/compound-assign branches; `fib(10)` now overflows the default
    // 1 MB test-thread stack. Run on a large-stack worker thread, consistent
    // with `test_factorial_100` and other deep-recursion tests.
    let src = r#"
        fn fib(n) -> Int {
            if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
        }
        fib(10)
    "#;
    assert_eq!(eval_big(src), "55");
}

#[test]
fn test_closure_captures_environment() {
    let src = r#"
        fn make_adder(x) -> fn(Int) -> Int {
            fn(y) { x + y }
        }
        let add5 = make_adder(5);
        add5(10)
    "#;
    assert_eq!(eval(src), eval("15"));
}

#[test]
fn test_lambda_expression() {
    let src = r#"
        let double = fn(x) { x * 2 };
        double(21)
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_block_scope() {
    let src = r#"
        let x = 1;
        {
            let x = 99;
            x
        }
    "#;
    assert_eq!(eval(src), eval("99"));
}

// ---------------------------------------------------------------------------
// Control flow
// ---------------------------------------------------------------------------

#[test]
fn test_if_expression() {
    assert_eq!(eval("if true { 1 } else { 2 }"), eval("1"));
    assert_eq!(eval("if false { 1 } else { 2 }"), eval("2"));
    assert_eq!(eval("if 1 > 0 { 42 }"), eval("42"));
}

#[test]
fn test_match_with_variants() {
    let src = r#"
        enum Option { Some(Int), None }
        match Some(42) {
            Some(x) => x,
            None => 0
        }
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_match_none() {
    let src = r#"
        enum Option { Some(Int), None }
        match None {
            Some(x) => x,
            None => 0
        }
    "#;
    assert_eq!(eval(src), eval("0"));
}

#[test]
fn test_match_with_guard() {
    let src = r#"
        match 10 {
            x if x > 5 => "big",
            _ => "small"
        }
    "#;
    match eval(src) {
        Value::Str(s) => assert_eq!(&**s, "big"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_match_or_pattern() {
    let src = r#"
        match 3 {
            1 | 2 | 3 => "low",
            _ => "high"
        }
    "#;
    match eval(src) {
        Value::Str(s) => assert_eq!(&**s, "low"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_match_vec_pattern() {
    let src = r#"
        match [1, 2, 3] {
            [a, b, c] => a + b + c,
            _ => 0
        }
    "#;
    assert_eq!(eval(src), eval("6"));
}

#[test]
fn test_match_vec_with_rest() {
    let src = r#"
        match [1, 2, 3, 4, 5] {
            [first, ..] => first,
            _ => 0
        }
    "#;
    assert_eq!(eval(src), eval("1"));
}

#[test]
fn test_match_struct_pattern() {
    let src = r#"
        type Point = { x: Int, y: Int }
        let p = Point({ x: 3, y: 4 });
        match p {
            Point { x: px, y: py } => px * py,
            _ => 0
        }
    "#;
    assert_eq!(eval(src), eval("12"));
}

// ---------------------------------------------------------------------------
// Exceptions: try / raise / rescue
// ---------------------------------------------------------------------------

#[test]
fn test_raise_and_rescue() {
    let src = r#"
        try {
            raise 42
        } rescue as e {
            e
        }
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_raise_with_variant_rescue() {
    let src = r#"
        enum Result { Ok(Int), Err(String) }
        try {
            raise Err("not found")
        } rescue Err as e {
            e
        } rescue as other {
            other
        }
    "#;
    // The result should be the Err variant
    match eval(src) {
        Value::Variant { name, .. } => {
            assert_eq!(&**name, "Err");
        }
        other => panic!("expected Variant, got {:?}", other),
    }
}

#[test]
fn test_uncaught_exception_propagates() {
    let err = eval_err("raise 42");
    assert!(err.contains("uncaught exception"));
}

#[test]
fn test_try_without_exception() {
    let src = r#"
        try {
            1 + 2
        } rescue as e {
            0
        }
    "#;
    assert_eq!(eval(src), eval("3"));
}

// ---------------------------------------------------------------------------
// Pipe operator
// ---------------------------------------------------------------------------

#[test]
fn test_pipe_to_function() {
    let src = r#"
        let double = fn(x) { x * 2 };
        let inc = fn(x) { x + 1 };
        5 |> double |> inc
    "#;
    assert_eq!(eval(src), eval("11"));
}

#[test]
fn test_pipe_to_call_with_args() {
    let src = r#"
        let add = fn(a, b) { a + b };
        5 |> add(10)
    "#;
    assert_eq!(eval(src), eval("15"));
}

// ---------------------------------------------------------------------------
// String operations
// ---------------------------------------------------------------------------

#[test]
fn test_string_concatenation() {
    match eval(r#""hello" + " " + "world""#) {
        Value::Str(s) => assert_eq!(&**s, "hello world"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_string_interpolation() {
    let src = r#"
        let name = "world";
        "hello {name}!"
    "#;
    match eval(src) {
        Value::Str(s) => assert_eq!(&**s, "hello world!"),
        other => panic!("expected Str, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Custom types: struct and enum
// ---------------------------------------------------------------------------

#[test]
fn test_struct_construction_and_field_access() {
    let src = r#"
        type Point = { x: Int, y: Int }
        let p = Point({ x: 3, y: 4 });
        p.x
    "#;
    assert_eq!(eval(src), eval("3"));
}

#[test]
fn test_enum_variants() {
    let src = r#"
        enum Color { Red, Green, Blue }
        Red
    "#;
    match eval(src) {
        Value::Variant { name, args } => {
            assert_eq!(&**name, "Red");
            assert!(args.is_empty());
        }
        other => panic!("expected Variant, got {:?}", other),
    }
}

#[test]
fn test_enum_with_args() {
    let src = r#"
        enum Shape { Circle(Int), Rect(Int, Int) }
        let c = Circle(5);
        match c {
            Circle(r) => r * r,
            Rect(w, h) => w * h
        }
    "#;
    assert_eq!(eval(src), eval("25"));
}

// ---------------------------------------------------------------------------
// Assignment
// ---------------------------------------------------------------------------

#[test]
fn test_variable_assignment() {
    let src = r#"
        let x = 1;
        x = 10;
        x
    "#;
    assert_eq!(eval(src), eval("10"));
}

#[test]
fn test_index_assignment() {
    let src = r#"
        let v = [1, 2, 3];
        v[0] = 99;
        v[0]
    "#;
    assert_eq!(eval(src), eval("99"));
}

#[test]
fn test_field_assignment() {
    let src = r#"
        type Point = { x: Int, y: Int }
        let p = Point({ x: 1, y: 2 });
        p.x = 42;
        p.x
    "#;
    assert_eq!(eval(src), eval("42"));
}

// ---------------------------------------------------------------------------
// Built-in functions
// ---------------------------------------------------------------------------

#[test]
fn test_builtin_type_predicates() {
    assert_eq!(eval("is_int(42)"), Value::Bool(true));
    assert_eq!(eval("is_decimal(3.14)"), Value::Bool(true));
    assert_eq!(eval("is_str(\"hi\")"), Value::Bool(true));
    assert_eq!(eval("is_vec([1, 2])"), Value::Bool(true));
    assert_eq!(eval("is_nil(nil)"), Value::Bool(true));
    assert_eq!(eval("is_number(42)"), Value::Bool(true));
    assert_eq!(eval("is_number(3.14)"), Value::Bool(true));
    assert_eq!(eval("is_number(\"hi\")"), Value::Bool(false));
}

#[test]
fn test_builtin_type_of() {
    match eval("type_of(42)") {
        Value::Str(s) => assert_eq!(&**s, "Int"),
        other => panic!("expected Str, got {:?}", other),
    }
    match eval("type_of(\"hi\")") {
        Value::Str(s) => assert_eq!(&**s, "String"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_builtin_conversions() {
    assert_eq!(eval("int(3.7)"), eval("3"));
    assert_eq!(eval("decimal(5)"), eval("5.0"));
    match eval("str(42)") {
        Value::Str(s) => assert_eq!(&**s, "42"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_builtin_abs() {
    assert_eq!(eval("abs(-5)"), eval("5"));
    assert_eq!(eval("abs(5)"), eval("5"));
}

#[test]
fn test_method_call_syntax() {
    // `xs.count()` desugars to `count(xs)`
    assert_eq!(eval("[1, 2, 3].count()"), eval("3"));
    assert_eq!(eval("[1, 2, 3].first()"), eval("1"));
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_name_error() {
    let err = eval_err("undefined_var");
    assert!(err.contains("not defined"));
}

#[test]
fn test_division_by_zero() {
    let err = eval_err("1 / 0");
    assert!(err.contains("division by zero"));
}

#[test]
fn test_type_error() {
    let err = eval_err("1 + \"hello\"");
    assert!(err.contains("type error"));
}

#[test]
fn test_arity_error() {
    let err = eval_err(r#"fn f(x) { x } f(1, 2)"#);
    assert!(err.contains("expects") || err.contains("argument"));
}

#[test]
fn test_pattern_match_fail() {
    let err = eval_err("match 42 { 1 => 1 }");
    assert!(err.contains("no pattern matched") || err.contains("match"));
}
