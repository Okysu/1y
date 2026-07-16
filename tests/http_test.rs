//! Tests for the self-hosted HTTP library (lib/http.1y).
//!
//! These tests load the pure-1y HTTP library via the module system and
//! exercise its `parse_request` and `response` functions. The `serve`
//! function is not tested here because it blocks forever.

use onely::Interpreter;
use onely::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Locate the project root (the directory containing Cargo.toml).
fn project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
}

static SETUP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Set up a temp entry directory containing a `lib/http.1y` so that
/// `import lib.http` resolves correctly.
///
/// Each call uses a unique subdirectory (keyed by an atomic counter) so that
/// tests running in parallel don't race on the same file.
fn setup_http_module() -> PathBuf {
    let root = project_root();
    let src = root.join("lib").join("http.1y");
    let content = fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", src.display(), e));

    let id = SETUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("onely_tests")
        .join("http_lib")
        .join(format!("t{}", id));
    let lib_dir = dir.join("lib");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(lib_dir.join("http.1y"), &content).unwrap();
    dir
}

fn eval_with_http(src: &str) -> Value {
    let dir = setup_http_module();
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

// ---------------------------------------------------------------------------
// parse_request
// ---------------------------------------------------------------------------

#[test]
fn test_parse_request_basic_get() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = http.parse_request(raw);
        get(req, "method")
    "#);
    assert_eq!(v, Value::str("GET"));
}

#[test]
fn test_parse_request_path() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "GET /api/users/42 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let req = http.parse_request(raw);
        get(req, "path")
    "#);
    assert_eq!(v, Value::str("/api/users/42"));
}

#[test]
fn test_parse_request_version() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "POST /submit HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let req = http.parse_request(raw);
        get(req, "version")
    "#);
    assert_eq!(v, Value::str("HTTP/1.1"));
}

#[test]
fn test_parse_request_body() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "POST /submit HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let req = http.parse_request(raw);
        get(req, "body")
    "#);
    assert_eq!(v, Value::str("hello"));
}

#[test]
fn test_parse_request_header() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "GET / HTTP/1.1\r\nHost: example.com\r\nAccept: text/html\r\n\r\n";
        let req = http.parse_request(raw);
        let headers = get(req, "headers");
        get(headers, "Host")
    "#);
    assert_eq!(v, Value::str("example.com"));
}

#[test]
fn test_parse_request_malformed_returns_nil() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let raw = "not a valid http request";
        http.parse_request(raw)
    "#);
    assert_eq!(v, Value::Nil);
}

// ---------------------------------------------------------------------------
// response builder
// ---------------------------------------------------------------------------

#[test]
fn test_response_contains_status_line() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.response(200, "hello", []);
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("HTTP/1.1 200 OK"), "got: {}", s);
}

#[test]
fn test_response_contains_content_length() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.response(200, "hello", []);
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("Content-Length: 5"), "got: {}", s);
}

#[test]
fn test_response_contains_body() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.response(200, "hello world", []);
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("hello world"), "got: {}", s);
}

#[test]
fn test_response_with_headers() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.response(201, "created", ["Content-Type: application/json"]);
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("Content-Type: application/json"), "got: {}", s);
    assert!(s.contains("HTTP/1.1 201 Created"), "got: {}", s);
}

#[test]
fn test_json_response() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.json_response(200, "\{ \"ok\": true \}");
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("Content-Type: application/json"), "got: {}", s);
}

#[test]
fn test_not_found_response() {
    let v = eval_with_http(r#"
        import lib.http as http;
        let resp = http.not_found();
        resp
    "#);
    let s = format!("{}", v);
    assert!(s.contains("404 Not Found"), "got: {}", s);
}

// ---------------------------------------------------------------------------
// status_text
// ---------------------------------------------------------------------------

#[test]
fn test_status_text_known() {
    let v = eval_with_http(r#"
        import lib.http as http;
        http.status_text(200)
    "#);
    assert_eq!(v, Value::str("OK"));
}

#[test]
fn test_status_text_unknown() {
    let v = eval_with_http(r#"
        import lib.http as http;
        http.status_text(999)
    "#);
    assert_eq!(v, Value::str("Unknown"));
}
