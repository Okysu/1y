//! Phase 3.5a tests: loops (`while` / `for...in` / `loop` / `break` / `continue`)
//! and compound assignment (`+=` / `-=` / `*=` / `/=` / `%=`).
//!
//! Loops return `Nil`; their effect is observed through mutable variables.
//! `for...in` iterates Vec (elements), Set (elements), Map (`[k, v]` pairs),
//! and Str (characters).

use onely::Interpreter;
use onely::Value;

fn eval(src: &str) -> Value {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

fn eval_str(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => (*s).clone(),
        other => panic!("expected Str, got {:?}", other),
    }
}

fn eval_err(src: &str) -> String {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(_) => panic!("expected error, got success"),
        Err(e) => format!("{}", e),
    }
}

// ---------------------------------------------------------------------------
// while
// ---------------------------------------------------------------------------

#[test]
fn test_while_basic_counter() {
    let src = r#"
        let i = 0;
        let sum = 0;
        while i < 10 {
            sum = sum + i;
            i = i + 1
        };
        sum
    "#;
    assert_eq!(eval(src), eval("45")); // 0+1+..+9
}

#[test]
fn test_while_returns_nil() {
    let src = r#"
        while false { 1 }
    "#;
    assert_eq!(eval(src), Value::Nil);
}

#[test]
fn test_while_body_not_executed_when_cond_false() {
    let src = r#"
        let x = 0;
        while false { x = 999 };
        x
    "#;
    assert_eq!(eval(src), eval("0"));
}

#[test]
fn test_while_with_break() {
    let src = r#"
        let i = 0;
        while true {
            if i >= 5 { break };
            i = i + 1
        };
        i
    "#;
    assert_eq!(eval(src), eval("5"));
}

#[test]
fn test_while_with_continue() {
    // Sum odd numbers 1..10: skip even via continue.
    let src = r#"
        let i = 0;
        let sum = 0;
        while i < 10 {
            i = i + 1;
            if i % 2 == 0 { continue };
            sum = sum + i
        };
        sum
    "#;
    assert_eq!(eval(src), eval("25")); // 1+3+5+7+9
}

#[test]
fn test_while_continue_skips_rest_of_body() {
    // After `continue`, the rest of the body must not run.
    let src = r#"
        let i = 0;
        let log = 0;
        while i < 3 {
            i = i + 1;
            continue;
            log = 999
        };
        log
    "#;
    assert_eq!(eval(src), eval("0"));
}

// ---------------------------------------------------------------------------
// for ... in
// ---------------------------------------------------------------------------

#[test]
fn test_for_over_vec() {
    let src = r#"
        let sum = 0;
        for x in [1, 2, 3, 4, 5] {
            sum = sum + x
        };
        sum
    "#;
    assert_eq!(eval(src), eval("15"));
}

#[test]
fn test_for_over_vec_break() {
    let src = r#"
        let sum = 0;
        for x in [1, 2, 3, 4, 5] {
            if x > 3 { break };
            sum = sum + x
        };
        sum
    "#;
    assert_eq!(eval(src), eval("6")); // 1+2+3
}

#[test]
fn test_for_over_vec_continue() {
    let src = r#"
        let sum = 0;
        for x in [1, 2, 3, 4, 5] {
            if x % 2 == 0 { continue };
            sum = sum + x
        };
        sum
    "#;
    assert_eq!(eval(src), eval("9")); // 1+3+5
}

#[test]
fn test_for_over_set() {
    // Set order is arbitrary; test sum which is order-independent.
    let src = r#"
        let sum = 0;
        for x in #{1, 2, 3, 4, 5} {
            sum = sum + x
        };
        sum
    "#;
    assert_eq!(eval(src), eval("15"));
}

#[test]
fn test_for_over_map() {
    // Each iteration yields a [key, value] pair.
    let src = r#"
        let m = { "a": 1, "b": 2, "c": 3 };
        let sum = 0;
        for kv in m {
            sum = sum + kv[1]
        };
        sum
    "#;
    assert_eq!(eval(src), eval("6")); // 1+2+3
}

#[test]
fn test_for_over_str_chars() {
    let src = r#"
        let out = "";
        for c in "abc" {
            out = out + c
        };
        out
    "#;
    assert_eq!(eval_str(src), "abc");
}

#[test]
fn test_for_loop_var_scoped_to_body() {
    // The loop variable must not leak to the outer scope.
    let src = r#"
        let x = 0;
        for x in [99, 100] { x };
        x
    "#;
    assert_eq!(eval(src), eval("0"));
}

#[test]
fn test_for_over_empty_vec() {
    let src = r#"
        let sum = 0;
        for x in [] { sum = sum + x };
        sum
    "#;
    assert_eq!(eval(src), eval("0"));
}

#[test]
fn test_for_over_non_iterable_errors() {
    let err = eval_err("for x in 42 { x }");
    assert!(err.contains("iterable") || err.contains("type"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// loop / break / continue
// ---------------------------------------------------------------------------

#[test]
fn test_loop_with_break() {
    let src = r#"
        let i = 0;
        loop {
            if i >= 10 { break };
            i = i + 1
        };
        i
    "#;
    assert_eq!(eval(src), eval("10"));
}

#[test]
fn test_loop_break_returns_value() {
    // `break expr` makes `loop` evaluate to expr.
    let src = r#"
        let i = 0;
        let result = loop {
            if i >= 5 { break i * 2 };
            i = i + 1
        };
        result
    "#;
    assert_eq!(eval(src), eval("10"));
}

#[test]
fn loop_returns_nil_on_bare_break() {
    let src = r#"
        loop { break }
    "#;
    assert_eq!(eval(src), Value::Nil);
}

#[test]
fn test_loop_with_continue() {
    // Count iterations that actually increment (skip evens).
    let src = r#"
        let i = 0;
        let hits = 0;
        loop {
            i = i + 1;
            if i > 10 { break };
            if i % 2 == 0 { continue };
            hits = hits + 1
        };
        hits
    "#;
    assert_eq!(eval(src), eval("5")); // 1,3,5,7,9
}

#[test]
fn test_break_outside_loop_errors() {
    let err = eval_err("break");
    assert!(err.contains("break"), "got: {}", err);
}

#[test]
fn test_continue_outside_loop_errors() {
    let err = eval_err("continue");
    assert!(err.contains("continue"), "got: {}", err);
}

#[test]
fn test_break_value_outside_loop_errors() {
    let err = eval_err("break 42");
    assert!(err.contains("break"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// Nested loops
// ---------------------------------------------------------------------------

#[test]
fn test_nested_while() {
    // Multiplication table sum: i*j for i in 1..3, j in 1..3
    let src = r#"
        let i = 1;
        let total = 0;
        while i <= 3 {
            let j = 1;
            while j <= 3 {
                total = total + i * j;
                j = j + 1
            };
            i = i + 1
        };
        total
    "#;
    // (1+2+3) + (2+4+6) + (3+6+9) = 6 + 12 + 18 = 36
    assert_eq!(eval(src), eval("36"));
}

#[test]
fn test_break_only_exits_inner_loop() {
    let src = r#"
        let i = 0;
        let outer_runs = 0;
        while i < 3 {
            outer_runs = outer_runs + 1;
            let j = 0;
            while true {
                if j >= 2 { break };
                j = j + 1
            };
            i = i + 1
        };
        outer_runs
    "#;
    assert_eq!(eval(src), eval("3"));
}

#[test]
fn test_nested_for() {
    let src = r#"
        let total = 0;
        for i in [1, 2] {
            for j in [10, 20] {
                total = total + i * j
            }
        };
        total
    "#;
    // (1*10 + 1*20) + (2*10 + 2*20) = 30 + 60 = 90
    assert_eq!(eval(src), eval("90"));
}

// ---------------------------------------------------------------------------
// Compound assignment ( += -= *= /= %= )
// ---------------------------------------------------------------------------

#[test]
fn test_compound_add() {
    let src = r#"
        let x = 10;
        x += 5;
        x
    "#;
    assert_eq!(eval(src), eval("15"));
}

#[test]
fn test_compound_sub() {
    let src = r#"
        let x = 10;
        x -= 3;
        x
    "#;
    assert_eq!(eval(src), eval("7"));
}

#[test]
fn test_compound_mul() {
    let src = r#"
        let x = 6;
        x *= 7;
        x
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_compound_div_preserves_int_when_even() {
    let src = r#"
        let x = 12;
        x /= 3;
        x
    "#;
    assert_eq!(eval(src), eval("4"));
}

#[test]
fn test_compound_div_promotes_to_decimal() {
    let src = r#"
        let x = 7;
        x /= 2;
        x
    "#;
    match eval(src) {
        Value::Decimal(_) => {}
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_compound_mod() {
    let src = r#"
        let x = 17;
        x %= 5;
        x
    "#;
    assert_eq!(eval(src), eval("2"));
}

#[test]
fn test_compound_add_string_concat() {
    let src = r#"
        let s = "hello";
        s += " world";
        s
    "#;
    assert_eq!(eval_str(src), "hello world");
}

#[test]
fn test_compound_assign_to_vec_index() {
    let src = r#"
        let v = [1, 2, 3];
        v[1] += 10;
        v[1]
    "#;
    assert_eq!(eval(src), eval("12"));
}

#[test]
fn test_compound_assign_to_struct_field() {
    let src = r#"
        type Point = { x: Int, y: Int };
        let p = Point({ x: 1, y: 2 });
        p.x += 10;
        p.x
    "#;
    assert_eq!(eval(src), eval("11"));
}

#[test]
fn test_compound_assign_chained_in_loop() {
    // Accumulate sum and product in a loop using compound assignment.
    let src = r#"
        let sum = 0;
        let prod = 1;
        for x in [1, 2, 3, 4] {
            sum += x;
            prod *= x
        };
        sum
    "#;
    assert_eq!(eval(src), eval("10")); // 1+2+3+4
}

#[test]
fn test_compound_assign_chained_in_loop_product() {
    let src = r#"
        let prod = 1;
        for x in [1, 2, 3, 4] {
            prod *= x
        };
        prod
    "#;
    assert_eq!(eval(src), eval("24")); // 1*2*3*4
}

// ---------------------------------------------------------------------------
// Loop as expression (loop/break returns value)
// ---------------------------------------------------------------------------

#[test]
fn test_loop_as_expression() {
    // `loop { ... break v }` is an expression yielding v.
    let src = r#"
        let n = 0;
        let x = loop {
            n += 1;
            if n >= 3 { break n * 10 } else { continue }
        };
        x
    "#;
    assert_eq!(eval(src), eval("30"));
}

#[test]
fn test_while_does_not_return_break_value() {
    // `while` always returns Nil even if `break expr` is used.
    let src = r#"
        let r = while true { break 42 };
        r
    "#;
    assert_eq!(eval(src), Value::Nil);
}

#[test]
fn test_for_does_not_return_break_value() {
    // `for` always returns Nil even if `break expr` is used.
    let src = r#"
        let r = for x in [1, 2, 3] { break 42 };
        r
    "#;
    assert_eq!(eval(src), Value::Nil);
}

// ---------------------------------------------------------------------------
// Practical: loops + closures
// ---------------------------------------------------------------------------

#[test]
fn test_loop_replaces_recursion_for_factorial() {
    // Iterative factorial via while — no recursion, no stack overflow risk.
    let src = r#"
        fn fact_iter(n) -> Int {
            let acc = 1;
            let i = 1;
            while i <= n {
                acc *= i;
                i += 1
            };
            acc
        };
        fact_iter(20)
    "#;
    assert_eq!(eval(src), eval("2432902008176640000"));
}

#[test]
fn test_loop_builds_vec_via_push() {
    let src = r#"
        let xs = [];
        let i = 0;
        while count(xs) < 5 {
            xs = push(xs, i);
            i += 1
        };
        xs[4]
    "#;
    assert_eq!(eval(src), eval("4"));
}

#[test]
fn test_for_accumulates_into_map() {
    // Build a Map by assoc'ing into an initial non-empty Map (the parser
    // treats `{}` as an empty block, so start from `{ "init": 0 }`).
    let src = r#"
        let m = { "init": 0 };
        let i = 0;
        for x in ["a", "b", "c"] {
            m = assoc(m, x, i);
            i += 1
        };
        get(m, "b")
    "#;
    assert_eq!(eval(src), eval("1"));
}
