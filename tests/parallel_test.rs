//! Integration tests for the `parallel` module (multi-threading).

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

/// Source defining both `double` and `greet` so every pool-setting test
/// shares the same function set (avoids collisions when tests run in
/// parallel and overwrite the global pool).
const POOL_SRC: &str = r#"
    fn double(n) { n * 2 }
    fn greet(name) { "hello " + name }
"#;

fn setup_pool(n: usize) {
    let pool = onely::runtime::worker::WorkerPool::new(n, Some(POOL_SRC.into()), None);
    onely::runtime::worker::set_global_pool(pool);
}

#[test]
fn test_parallel_cores() {
    let v = eval("parallel.cores()");
    match v {
        Value::Int(n) => {
            assert!(n > 0.into(), "cores should be > 0");
        }
        other => panic!("expected Int, got {:?}", other),
    }
}

#[test]
fn test_parallel_call() {
    setup_pool(2);
    let v = eval(r#"parallel.call("double", [21])"#);
    assert_eq!(v, Value::int(42));
}

#[test]
fn test_parallel_spawn_and_join() {
    setup_pool(2);
    let v = eval(
        r#"
        let h = parallel.spawn("double", [21]);
        parallel.join(h)
    "#,
    );
    assert_eq!(v, Value::int(42));
}

#[test]
fn test_parallel_map() {
    setup_pool(4);
    let v = eval(r#"parallel.map("double", [[1], [2], [3], [4]])"#);
    match v {
        Value::Vec(items) => {
            assert_eq!(items.len(), 4);
            assert_eq!(items[0], Value::int(2));
            assert_eq!(items[1], Value::int(4));
            assert_eq!(items[2], Value::int(6));
            assert_eq!(items[3], Value::int(8));
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_parallel_call_with_string_arg() {
    setup_pool(2);
    let v = eval(r#"parallel.call("greet", ["world"])"#);
    assert_eq!(v, Value::str("hello world"));
}

#[test]
fn test_parallel_spawn_with_string_arg() {
    setup_pool(2);
    let v = eval(
        r#"
        let h = parallel.spawn("greet", ["world"]);
        parallel.join(h)
    "#,
    );
    assert_eq!(v, Value::str("hello world"));
}

#[test]
fn test_parallel_call_undefined_function() {
    setup_pool(1);
    let err = eval_err(r#"parallel.call("nonexistent", [])"#);
    assert!(
        err.contains("not defined") || err.contains("nonexistent"),
        "expected 'not defined' error, got: {}",
        err
    );
}
