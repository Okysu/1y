//! Phase 3.5d tests: math functions.
//!
//! `min` / `max` / `floor` / `ceil` / `round` / `sqrt` / `sin` / `cos` / `log` / `exp`.

use onely::Interpreter;
use onely::Value;
use num_traits::ToPrimitive;

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

// ---------------------------------------------------------------------------
// min / max
// ---------------------------------------------------------------------------

#[test]
fn test_min_int() {
    assert_eq!(eval("min(3, 5)"), eval("3"));
    assert_eq!(eval("min(5, 3)"), eval("3"));
    assert_eq!(eval("min(-5, -3)"), eval("-5"));
}

#[test]
fn test_max_int() {
    assert_eq!(eval("max(3, 5)"), eval("5"));
    assert_eq!(eval("max(5, 3)"), eval("5"));
    assert_eq!(eval("max(-5, -3)"), eval("-3"));
}

#[test]
fn test_min_mixed_types() {
    // min(3, 3.5) → 3 (Int, the smaller one)
    match eval("min(3, 3.5)") {
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int, got {:?}", other),
    }
    // min(4.5, 3) → 3 (Int, the smaller one)
    match eval("min(4.5, 3)") {
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int, got {:?}", other),
    }
}

#[test]
fn test_max_mixed_types() {
    // max(3, 3.5) → 3.5 (Decimal)
    match eval("max(3, 3.5)") {
        Value::Decimal(_) => {}
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_min_equal() {
    assert_eq!(eval("min(7, 7)"), eval("7"));
}

// ---------------------------------------------------------------------------
// floor / ceil / round
// ---------------------------------------------------------------------------

#[test]
fn test_floor() {
    assert_eq!(eval("floor(3.7)"), eval("3"));
    assert_eq!(eval("floor(3.2)"), eval("3"));
    assert_eq!(eval("floor(-3.2)"), eval("-4"));
}

#[test]
fn test_ceil() {
    assert_eq!(eval("ceil(3.2)"), eval("4"));
    assert_eq!(eval("ceil(3.0)"), eval("3"));
    assert_eq!(eval("ceil(-3.7)"), eval("-3"));
}

#[test]
fn test_round() {
    assert_eq!(eval("round(3.4)"), eval("3"));
    assert_eq!(eval("round(3.5)"), eval("4"));
    assert_eq!(eval("round(3.6)"), eval("4"));
    assert_eq!(eval("round(-3.5)"), eval("-4"));
}

#[test]
fn test_floor_ceil_int_passthrough() {
    // Int values pass through unchanged.
    assert_eq!(eval("floor(42)"), eval("42"));
    assert_eq!(eval("ceil(42)"), eval("42"));
    assert_eq!(eval("round(42)"), eval("42"));
}

#[test]
fn test_floor_method_syntax() {
    assert_eq!(eval("3.7.floor()"), eval("3"));
}

// ---------------------------------------------------------------------------
// sqrt
// ---------------------------------------------------------------------------

#[test]
fn test_sqrt_perfect() {
    match eval("sqrt(4)") {
        Value::Decimal(d) => {
            let f = d.to_f64().unwrap();
            assert!((f - 2.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_sqrt_non_perfect() {
    match eval("sqrt(2)") {
        Value::Decimal(d) => {
            let f = d.to_f64().unwrap();
            assert!((f - 1.41421356).abs() < 1e-6);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_sqrt_zero() {
    match eval("sqrt(0)") {
        Value::Decimal(d) => {
            assert_eq!(d.to_f64().unwrap(), 0.0);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_sqrt_negative_errors() {
    let err = eval_err("sqrt(-1)");
    assert!(err.contains("non-finite") || err.contains("NaN"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// sin / cos
// ---------------------------------------------------------------------------

#[test]
fn test_sin_zero() {
    match eval("sin(0)") {
        Value::Decimal(d) => {
            assert!(d.to_f64().unwrap().abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_cos_zero() {
    match eval("cos(0)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 1.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_sin_pi() {
    // sin(pi) ≈ 0
    match eval("sin(3.14159265358979)") {
        Value::Decimal(d) => {
            assert!(d.to_f64().unwrap().abs() < 1e-9, "sin(pi) should be ~0");
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_cos_pi() {
    // cos(pi) ≈ -1
    match eval("cos(3.14159265358979)") {
        Value::Decimal(d) => {
            let f = d.to_f64().unwrap();
            assert!((f - (-1.0)).abs() < 1e-9, "cos(pi) should be ~-1, got {}", f);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// log
// ---------------------------------------------------------------------------

#[test]
fn test_log_base_10() {
    // log(100, 10) = 2
    match eval("log(100, 10)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 2.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_log_base_2() {
    // log(8, 2) = 3
    match eval("log(8, 2)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 3.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_log_natural() {
    // log(e, e) = 1
    match eval("log(2.718281828, 2.718281828)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 1.0).abs() < 1e-5);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_log_invalid_base() {
    let err = eval_err("log(10, 1)");
    assert!(err.contains("base") || err.contains("log"), "got: {}", err);
}

#[test]
fn test_log_negative_errors() {
    let err = eval_err("log(-1, 10)");
    assert!(err.contains("log"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// exp
// ---------------------------------------------------------------------------

#[test]
fn test_exp_zero() {
    // exp(0) = 1
    match eval("exp(0)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 1.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_exp_one() {
    // exp(1) ≈ e ≈ 2.71828
    match eval("exp(1)") {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 2.718281828).abs() < 1e-6);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Composed math
// ---------------------------------------------------------------------------

#[test]
fn test_pythagorean_theorem() {
    // sqrt(3^2 + 4^2) = 5
    let src = r#"
        sqrt(9 + 16)
    "#;
    match eval(src) {
        Value::Decimal(d) => {
            assert!((d.to_f64().unwrap() - 5.0).abs() < 1e-9);
        }
        other => panic!("expected Decimal, got {:?}", other),
    }
}

#[test]
fn test_min_max_in_loop() {
    let src = r#"
        let nums = [3, 1, 4, 1, 5, 9, 2, 6];
        let lo = nums[0];
        let hi = nums[0];
        for x in nums {
            lo = min(lo, x);
            hi = max(hi, x)
        };
        lo + hi
    "#;
    // min=1, max=9, sum=10
    assert_eq!(eval(src), eval("10"));
}

#[test]
fn test_floor_ceil_chain() {
    // floor(3.7) + ceil(3.2) = 3 + 4 = 7
    assert_eq!(eval("floor(3.7) + ceil(3.2)"), eval("7"));
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_min_wrong_type() {
    let err = eval_err(r#"min("a", 1)"#);
    assert!(err.contains("type") || err.contains("number"), "got: {}", err);
}

#[test]
fn test_sqrt_wrong_type() {
    let err = eval_err(r#"sqrt("hello")"#);
    assert!(err.contains("type") || err.contains("number"), "got: {}", err);
}

#[test]
fn test_floor_wrong_type() {
    let err = eval_err(r#"floor("hello")"#);
    assert!(err.contains("type") || err.contains("number"), "got: {}", err);
}
