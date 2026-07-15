//! Phase 3.5b tests: higher-order collection functions.
//!
//! `map` / `filter` / `fold` / `reduce` / `find` / `each` — each takes a
//! collection (Vec/Set/Map/Str) as the first argument and a closure as the
//! last, consistent with the `|>` pipe operator.

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
// map
// ---------------------------------------------------------------------------

#[test]
fn test_map_basic() {
    let src = r#"
        let double = fn(x) { x * 2 };
        map([1, 2, 3], double)
    "#;
    // Result is a Vec [2, 4, 6]; check by indexing.
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], eval("2"));
            assert_eq!(v[1], eval("4"));
            assert_eq!(v[2], eval("6"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_map_inline_lambda() {
    let src = r#"
        map([1, 2, 3], fn(x) { x + 10 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v[0], eval("11"));
            assert_eq!(v[2], eval("13"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_map_method_syntax() {
    let src = r#"
        [1, 2, 3].map(fn(x) { x * x })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v[0], eval("1"));
            assert_eq!(v[1], eval("4"));
            assert_eq!(v[2], eval("9"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_map_pipe_syntax() {
    let src = r#"
        [1, 2, 3] |> map(fn(x) { x + 1 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v[0], eval("2"));
            assert_eq!(v[2], eval("4"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_map_over_str() {
    let src = r#"
        let up = fn(c) { c + "!" };
        map("abc", up)
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3);
            match &v[0] {
                Value::Str(s) => assert_eq!(&***s, "a!"),
                other => panic!("expected Str, got {:?}", other),
            }
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_map_preserves_original() {
    let src = r#"
        let xs = [1, 2, 3];
        let ys = map(xs, fn(x) { x * 2 });
        xs[0]
    "#;
    assert_eq!(eval(src), eval("1")); // original unchanged
}

#[test]
fn test_map_empty() {
    let src = r#"
        map([], fn(x) { x })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => assert_eq!(v.len(), 0),
        other => panic!("expected Vec, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// filter
// ---------------------------------------------------------------------------

#[test]
fn test_filter_basic() {
    let src = r#"
        filter([1, 2, 3, 4, 5, 6], fn(x) { x % 2 == 0 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3);
            assert_eq!(v[0], eval("2"));
            assert_eq!(v[1], eval("4"));
            assert_eq!(v[2], eval("6"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_filter_method_syntax() {
    let src = r#"
        [1, 2, 3, 4].filter(fn(x) { x > 2 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], eval("3"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_filter_pipe_syntax() {
    let src = r#"
        [1, 2, 3, 4, 5] |> filter(fn(x) { x != 3 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => assert_eq!(v.len(), 4),
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_filter_all_removed() {
    let src = r#"
        filter([1, 2, 3], fn(x) { false })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => assert_eq!(v.len(), 0),
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_filter_all_kept() {
    let src = r#"
        filter([1, 2, 3], fn(x) { true })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => assert_eq!(v.len(), 3),
        other => panic!("expected Vec, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// fold
// ---------------------------------------------------------------------------

#[test]
fn test_fold_basic() {
    let src = r#"
        fold([1, 2, 3, 4], 0, fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("10")); // 1+2+3+4
}

#[test]
fn test_fold_with_nonzero_init() {
    let src = r#"
        fold([1, 2, 3], 100, fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("106")); // 100+1+2+3
}

#[test]
fn test_fold_method_syntax() {
    let src = r#"
        [1, 2, 3, 4].fold(1, fn(acc, x) { acc * x })
    "#;
    assert_eq!(eval(src), eval("24")); // 1*1*2*3*4
}

#[test]
fn test_fold_pipe_syntax() {
    let src = r#"
        [1, 2, 3] |> fold(0, fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("6"));
}

#[test]
fn test_fold_empty_returns_init() {
    let src = r#"
        fold([], 42, fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_fold_builds_vec() {
    let src = r#"
        let result = fold([1, 2, 3], [], fn(acc, x) { push(acc, x * 2) });
        result[2]
    "#;
    assert_eq!(eval(src), eval("6"));
}

#[test]
fn test_fold_builds_string() {
    let src = r#"
        fold(["a", "b", "c"], "", fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval_str(src), "abc");
}

// ---------------------------------------------------------------------------
// reduce
// ---------------------------------------------------------------------------

#[test]
fn test_reduce_basic() {
    let src = r#"
        reduce([1, 2, 3, 4], fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("10")); // ((1+2)+3)+4
}

#[test]
fn test_reduce_single_element() {
    let src = r#"
        reduce([42], fn(acc, x) { acc + x })
    "#;
    assert_eq!(eval(src), eval("42"));
}

#[test]
fn test_reduce_string_concat() {
    let src = r#"
        reduce(["hello", " ", "world"], fn(a, b) { a + b })
    "#;
    assert_eq!(eval_str(src), "hello world");
}

#[test]
fn test_reduce_empty_errors() {
    let err = eval_err("reduce([], fn(a, b) { a + b })");
    assert!(err.contains("empty"), "got: {}", err);
}

#[test]
fn test_reduce_method_syntax() {
    let src = r#"
        [10, 20, 30].reduce(fn(a, b) { a + b })
    "#;
    assert_eq!(eval(src), eval("60"));
}

// ---------------------------------------------------------------------------
// find
// ---------------------------------------------------------------------------

#[test]
fn test_find_returns_matching() {
    let src = r#"
        find([1, 2, 3, 4, 5], fn(x) { x > 3 })
    "#;
    assert_eq!(eval(src), eval("4"));
}

#[test]
fn test_find_first_match_only() {
    let src = r#"
        find([1, 2, 3, 4, 5], fn(x) { x % 2 == 0 })
    "#;
    assert_eq!(eval(src), eval("2")); // first even
}

#[test]
fn test_find_not_found_returns_nil() {
    let src = r#"
        find([1, 2, 3], fn(x) { x > 100 })
    "#;
    assert_eq!(eval(src), Value::Nil);
}

#[test]
fn test_find_method_syntax() {
    let src = r#"
        [10, 20, 30].find(fn(x) { x == 20 })
    "#;
    assert_eq!(eval(src), eval("20"));
}

#[test]
fn test_find_empty_returns_nil() {
    let src = r#"
        find([], fn(x) { true })
    "#;
    assert_eq!(eval(src), Value::Nil);
}

// ---------------------------------------------------------------------------
// each
// ---------------------------------------------------------------------------

#[test]
fn test_each_runs_side_effect() {
    let src = r#"
        let total = 0;
        each([1, 2, 3, 4], fn(x) { total = total + x });
        total
    "#;
    assert_eq!(eval(src), eval("10"));
}

#[test]
fn test_each_returns_nil() {
    let src = r#"
        each([1, 2, 3], fn(x) { x })
    "#;
    assert_eq!(eval(src), Value::Nil);
}

#[test]
fn test_each_method_syntax() {
    let src = r#"
        let count = 0;
        [1, 2, 3].each(fn(x) { count += 1 });
        count
    "#;
    assert_eq!(eval(src), eval("3"));
}

#[test]
fn test_each_over_map() {
    let src = r#"
        let sum = 0;
        each({ "a": 10, "b": 20 }, fn(kv) { sum += kv[1] });
        sum
    "#;
    assert_eq!(eval(src), eval("30"));
}

// ---------------------------------------------------------------------------
// Chained / composed usage
// ---------------------------------------------------------------------------

#[test]
fn test_chain_map_filter_pipe() {
    let src = r#"
        [1, 2, 3, 4, 5, 6]
            |> filter(fn(x) { x % 2 == 0 })
            |> map(fn(x) { x * 10 })
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3); // [20, 40, 60]
            assert_eq!(v[0], eval("20"));
            assert_eq!(v[1], eval("40"));
            assert_eq!(v[2], eval("60"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_chain_map_filter_fold() {
    let src = r#"
        [1, 2, 3, 4, 5, 6]
            |> map(fn(x) { x * x })
            |> filter(fn(x) { x > 10 })
            |> fold(0, fn(a, b) { a + b })
    "#;
    // squares: 1,4,9,16,25,36 → filter >10: 16,25,36 → sum: 77
    assert_eq!(eval(src), eval("77"));
}

#[test]
fn test_map_with_closure_capture() {
    let src = r#"
        fn make_multiplier(n) -> fn(Int) -> Int {
            fn(x) { x * n }
        };
        let triple = make_multiplier(3);
        map([1, 2, 3], triple)
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v[0], eval("3"));
            assert_eq!(v[1], eval("6"));
            assert_eq!(v[2], eval("9"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_filter_with_closure_capture() {
    let src = r#"
        let threshold = 5;
        let above = fn(x) { x > threshold };
        filter([1, 3, 5, 7, 9], above)
    "#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            // 5 is not > 5, so only [7, 9] pass.
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], eval("7"));
            assert_eq!(v[1], eval("9"));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_map_wrong_arity() {
    let err = eval_err("map([1, 2, 3])");
    assert!(err.contains("expects") || err.contains("argument"), "got: {}", err);
}

#[test]
fn test_map_non_callable() {
    let err = eval_err("map([1, 2, 3], 42)");
    assert!(err.contains("callable") || err.contains("type"), "got: {}", err);
}

#[test]
fn test_map_non_iterable() {
    let err = eval_err("map(42, fn(x) { x })");
    assert!(err.contains("iterable") || err.contains("type"), "got: {}", err);
}
