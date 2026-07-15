//! Phase 4 module system tests.
//!
//! Covers std module import, lazy import, alias, file module loading,
//! circular import detection, and error cases.

use onely::Interpreter;
use onely::Value;
use std::fs;
use std::path::PathBuf;

fn eval(src: &str) -> Value {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

fn eval_str(src: &str) -> String {
    format!("{}", eval(src))
}

fn eval_err(src: &str) -> String {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(_) => panic!("expected error, got success"),
        Err(e) => format!("{}", e),
    }
}

/// Create a temp directory with a `.1y` module file and return its path.
fn write_module(dir_name: &str, file_name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("onely_tests").join(dir_name);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file_name);
    fs::write(&path, content).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// Standard library module import
// ---------------------------------------------------------------------------

#[test]
fn test_import_std_module() {
    let v = eval(r#"import env; env"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_import_std_module_field_access() {
    // io.exists returns a Bool
    let v = eval(r#"import io; io.exists(".")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_import_std_module_field_access_missing() {
    let err = eval_err(r#"import io; io.nonexistent"#);
    assert!(err.contains("no export"), "got: {}", err);
}

#[test]
fn test_import_with_alias() {
    let v = eval(r#"import io as myio; myio.exists(".")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_import_binds_last_segment() {
    // `import io` binds `io`
    let v = eval(r#"import io; io"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_module_type_name() {
    let v = eval(r#"import io; type_of(io)"#);
    assert_eq!(v, Value::str("Module"));
}

#[test]
fn test_module_display() {
    // str(io) returns a Str, which displays with quotes.
    let v = eval_str(r#"import io; str(io)"#);
    assert_eq!(v, "\"<module io>\"");
}

// ---------------------------------------------------------------------------
// Lazy import
// ---------------------------------------------------------------------------

#[test]
fn test_lazy_import_defers_loading() {
    // Before access, the binding is a LazyImport placeholder.
    let v = eval(r#"lazy import io; io"#);
    // Accessing `io` triggers loading, so it becomes a Module.
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_lazy_import_resolves_on_field_access() {
    let v = eval(r#"lazy import io; io.exists(".")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_lazy_import_resolves_on_method_call() {
    // env.get returns Nil if the var is unset, or Str if set — either is fine.
    let v = eval(r#"lazy import env; env.get("HOME")"#);
    match v {
        Value::Nil | Value::Str(_) => {}
        _ => panic!("expected Nil or Str, got {}", v),
    }
}

// ---------------------------------------------------------------------------
// File module loading
// ---------------------------------------------------------------------------

#[test]
fn test_file_module_basic() {
    let dir = write_module(
        "file_basic",
        "mymod.1y",
        r#"let greeting = "hello from module";"#,
    );
    let src = format!(
        r#"import mymod; mymod.greeting"#,
    );
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    let v = interp.eval_source(&src).unwrap();
    assert_eq!(v, Value::str("hello from module"));
}

#[test]
fn test_file_module_with_functions() {
    let dir = write_module(
        "file_funcs",
        "maths.1y",
        r#"
            fn double(n) { n * 2 }
            fn triple(n) { n * 3 }
        "#,
    );
    let src = r#"
        import maths;
        maths.double(5) + maths.triple(5)
    "#;
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    let v = interp.eval_source(src).unwrap();
    assert_eq!(v, Value::int(25)); // 10 + 15
}

#[test]
fn test_file_module_cached() {
    let dir = write_module(
        "file_cached",
        "counter.1y",
        r#"let count = 42;"#,
    );
    let src = r#"
        import counter;
        import counter;
        counter.count
    "#;
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    let v = interp.eval_source(src).unwrap();
    assert_eq!(v, Value::int(42));
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_missing_module_error() {
    let err = eval_err(r#"import nonexistent.module.path"#);
    assert!(err.contains("import error"), "got: {}", err);
    assert!(err.contains("not found"), "got: {}", err);
}

#[test]
fn test_missing_export_error() {
    let v = eval_err(r#"import io; io.no_such_function"#);
    assert!(v.contains("no export"), "got: {}", v);
}

#[test]
fn test_circular_import_detection() {
    let dir = write_module("circular_a", "a.1y", r#"import b"#);
    // Write b.1y in the same directory
    fs::write(
        dir.join("b.1y"),
        r#"import a"#,
    ).unwrap();

    let src = r#"import a"#;
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    match interp.eval_source(src) {
        Ok(_) => panic!("expected circular import error"),
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("circular"), "got: {}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// Module isolation
// ---------------------------------------------------------------------------

#[test]
fn test_module_state_is_isolated() {
    let dir = write_module(
        "isolation",
        "stateful.1y",
        r#"let internal = 100;"#,
    );
    let src = r#"
        import stateful;
        // internal is NOT exported to the global scope
        internal
    "#;
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    match interp.eval_source(src) {
        Ok(_) => panic!("expected name error"),
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("not defined"), "got: {}", msg);
        }
    }
}
