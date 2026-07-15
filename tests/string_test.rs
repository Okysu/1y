//! Phase 3.5c tests: string operations.
//!
//! `len` / `split` / `join` / `replace` / `trim` / `contains` / `substring`.

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
// len
// ---------------------------------------------------------------------------

#[test]
fn test_len_ascii() {
    assert_eq!(eval(r#"len("hello")"#), eval("5"));
}

#[test]
fn test_len_unicode() {
    // Character count, not byte count: "héllo" = 5 chars, "你好" = 2 chars.
    assert_eq!(eval(r#"len("héllo")"#), eval("5"));
    assert_eq!(eval(r#"len("你好")"#), eval("2"));
}

#[test]
fn test_len_empty() {
    assert_eq!(eval(r#"len("")"#), eval("0"));
}

#[test]
fn test_len_vec() {
    assert_eq!(eval("len([1, 2, 3])"), eval("3"));
}

#[test]
fn test_len_method_syntax() {
    assert_eq!(eval(r#""hello".len()"#), eval("5"));
}

// ---------------------------------------------------------------------------
// split
// ---------------------------------------------------------------------------

#[test]
fn test_split_basic() {
    let src = r#"split("a,b,c", ",")"#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3);
            match &v[0] { Value::Str(s) => assert_eq!(&***s, "a"), _ => panic!() }
            match &v[2] { Value::Str(s) => assert_eq!(&***s, "c"), _ => panic!() }
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_split_empty_sep() {
    let src = r#"split("abc", "")"#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 3);
            match &v[0] { Value::Str(s) => assert_eq!(&***s, "a"), _ => panic!() }
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_split_no_match() {
    let src = r#"split("hello", ",")"#;
    let v = eval(src);
    match v {
        Value::Vec(v) => {
            assert_eq!(v.len(), 1);
            match &v[0] { Value::Str(s) => assert_eq!(&***s, "hello"), _ => panic!() }
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_split_method_syntax() {
    let src = r#""a-b-c".split("-")"#;
    let v = eval(src);
    match v {
        Value::Vec(v) => assert_eq!(v.len(), 3),
        other => panic!("expected Vec, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// join
// ---------------------------------------------------------------------------

#[test]
fn test_join_basic() {
    let src = r#"join(["a", "b", "c"], ", ")"#;
    assert_eq!(eval_str(src), "a, b, c");
}

#[test]
fn test_join_empty_sep() {
    let src = r#"join(["a", "b", "c"], "")"#;
    assert_eq!(eval_str(src), "abc");
}

#[test]
fn test_join_stringifies_numbers() {
    let src = r#"join([1, 2, 3], "-")"#;
    assert_eq!(eval_str(src), "1-2-3");
}

#[test]
fn test_join_single_element() {
    let src = r#"join(["only"], ",")"#;
    assert_eq!(eval_str(src), "only");
}

#[test]
fn test_join_method_syntax() {
    let src = r#"["a", "b"].join(",")"#;
    assert_eq!(eval_str(src), "a,b");
}

// ---------------------------------------------------------------------------
// replace
// ---------------------------------------------------------------------------

#[test]
fn test_replace_basic() {
    let src = r#"replace("hello world", "world", "there")"#;
    assert_eq!(eval_str(src), "hello there");
}

#[test]
fn test_replace_all_occurrences() {
    let src = r#"replace("a-b-a-b", "-", "+")"#;
    assert_eq!(eval_str(src), "a+b+a+b");
}

#[test]
fn test_replace_no_match() {
    let src = r#"replace("hello", "xyz", "abc")"#;
    assert_eq!(eval_str(src), "hello");
}

#[test]
fn test_replace_method_syntax() {
    let src = r#""hello".replace("l", "L")"#;
    assert_eq!(eval_str(src), "heLLo");
}

// ---------------------------------------------------------------------------
// trim
// ---------------------------------------------------------------------------

#[test]
fn test_trim_both_sides() {
    let src = r#"trim("  hello  ")"#;
    assert_eq!(eval_str(src), "hello");
}

#[test]
fn test_trim_tabs_newlines() {
    let src = r#"trim("\n\t  hi  \n")"#;
    assert_eq!(eval_str(src), "hi");
}

#[test]
fn test_trim_no_whitespace() {
    let src = r#"trim("hello")"#;
    assert_eq!(eval_str(src), "hello");
}

#[test]
fn test_trim_empty() {
    let src = r#"trim("   ")"#;
    assert_eq!(eval_str(src), "");
}

#[test]
fn test_trim_method_syntax() {
    let src = r#""  hi  ".trim()"#;
    assert_eq!(eval_str(src), "hi");
}

// ---------------------------------------------------------------------------
// contains
// ---------------------------------------------------------------------------

#[test]
fn test_contains_true() {
    assert_eq!(eval(r#"contains("hello world", "world")"#), Value::Bool(true));
}

#[test]
fn test_contains_false() {
    assert_eq!(eval(r#"contains("hello", "xyz")"#), Value::Bool(false));
}

#[test]
fn test_contains_empty_substring() {
    assert_eq!(eval(r#"contains("hello", "")"#), Value::Bool(true));
}

#[test]
fn test_contains_method_syntax() {
    assert_eq!(eval(r#""hello".contains("ell")"#), Value::Bool(true));
}

// ---------------------------------------------------------------------------
// substring
// ---------------------------------------------------------------------------

#[test]
fn test_substring_basic() {
    let src = r#"substring("hello", 1, 4)"#;
    assert_eq!(eval_str(src), "ell");
}

#[test]
fn test_substring_full() {
    let src = r#"substring("hello", 0, 5)"#;
    assert_eq!(eval_str(src), "hello");
}

#[test]
fn test_substring_empty_range() {
    let src = r#"substring("hello", 2, 2)"#;
    assert_eq!(eval_str(src), "");
}

#[test]
fn test_substring_end_clamped() {
    let src = r#"substring("hello", 1, 100)"#;
    assert_eq!(eval_str(src), "ello");
}

#[test]
fn test_substring_start_clamped() {
    let src = r#"substring("hello", 100, 200)"#;
    assert_eq!(eval_str(src), "");
}

#[test]
fn test_substring_unicode() {
    // "héllo" — character indices, not bytes.
    let src = r#"substring("héllo", 1, 4)"#;
    assert_eq!(eval_str(src), "éll");
}

#[test]
fn test_substring_method_syntax() {
    let src = r#""hello".substring(0, 3)"#;
    assert_eq!(eval_str(src), "hel");
}

// ---------------------------------------------------------------------------
// Composed operations
// ---------------------------------------------------------------------------

#[test]
fn test_chain_split_map_join() {
    // "1,2,3" → split → map(double) → join → "2,4,6"
    let src = r#"
        "1,2,3"
            |> split(",")
            |> map(fn(x) { str(int(x) * 2) })
            |> join(",")
    "#;
    assert_eq!(eval_str(src), "2,4,6");
}

#[test]
fn test_chain_trim_replace_upper() {
    // Trim then replace spaces with underscores.
    let src = r#"
        "  hello world  "
            |> trim()
            |> replace(" ", "_")
    "#;
    assert_eq!(eval_str(src), "hello_world");
}

#[test]
fn test_split_and_len() {
    let src = r#"
        len(split("a,b,c,d", ","))
    "#;
    assert_eq!(eval(src), eval("4"));
}

#[test]
fn test_contains_and_substring() {
    let src = r#"
        let s = "hello world";
        if contains(s, "world") {
            substring(s, 6, 11)
        } else {
            "not found"
        }
    "#;
    assert_eq!(eval_str(src), "world");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_len_wrong_type() {
    let err = eval_err("len(42)");
    assert!(err.contains("type") || err.contains("Str"), "got: {}", err);
}

#[test]
fn test_split_wrong_type() {
    let err = eval_err(r#"split(42, ",")"#);
    assert!(err.contains("type") || err.contains("Str"), "got: {}", err);
}

#[test]
fn test_join_wrong_type() {
    let err = eval_err(r#"join(42, ",")"#);
    assert!(err.contains("type") || err.contains("Vec"), "got: {}", err);
}

#[test]
fn test_substring_wrong_type() {
    let err = eval_err("substring(42, 0, 1)");
    assert!(err.contains("type") || err.contains("Str"), "got: {}", err);
}
