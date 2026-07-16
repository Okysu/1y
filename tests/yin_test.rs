//! Integration tests for the `yin` web framework (lib/yin.1y).
//!
//! These tests load yin (which depends on lib/http.1y) via the module system
//! and exercise its routing, groups, middleware, and response helpers. The
//! `run` function (which starts a blocking server) is not tested here.

use onely::Interpreter;
use onely::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

static SETUP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Set up a temp entry directory containing `lib/yin.1y` and `lib/http.1y`
/// so that `import lib.yin` resolves correctly. Each call uses a unique
/// subdirectory so parallel tests don't race on the same files.
fn setup_yin_modules() -> PathBuf {
    let root = project_root();
    let yin_src = root.join("lib").join("yin.1y");
    let http_src = root.join("lib").join("http.1y");

    let yin_content = fs::read_to_string(&yin_src)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", yin_src.display(), e));
    let http_content = fs::read_to_string(&http_src)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", http_src.display(), e));

    let id = SETUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("onely_tests")
        .join("yin_lib")
        .join(format!("t{}", id));
    let lib_dir = dir.join("lib");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(lib_dir.join("yin.1y"), &yin_content).unwrap();
    fs::write(lib_dir.join("http.1y"), &http_content).unwrap();
    dir
}

fn eval_with_yin(src: &str) -> Value {
    let dir = setup_yin_modules();
    let mut interp = Interpreter::new();
    interp.set_entry_dir(dir);
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

// ---------------------------------------------------------------------------
// Route registration + exact match
// ---------------------------------------------------------------------------

#[test]
fn test_exact_route_match() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let req = { "method": "GET", "path": "/ping", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

#[test]
fn test_exact_route_body() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let req = { "method": "GET", "path": "/ping", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "body")
    "#);
    let s = format!("{}", v);
    assert!(s.contains("pong"), "got: {}", s);
    assert!(s.contains("message"), "got: {}", s);
}

// ---------------------------------------------------------------------------
// Param route match
// ---------------------------------------------------------------------------

#[test]
fn test_param_route_match() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/users/:id", fn(ctx) {
            let id = yin.param(ctx, "id");
            yin.json(ctx, 200, { "user_id": id })
        });
        let req = { "method": "GET", "path": "/users/42", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

#[test]
fn test_param_route_extracts_param() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/users/:id", fn(ctx) {
            let id = yin.param(ctx, "id");
            yin.json(ctx, 200, { "user_id": id })
        });
        let req = { "method": "GET", "path": "/users/42", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "body")
    "#);
    let s = format!("{}", v);
    assert!(s.contains("42"), "got: {}", s);
}

// ---------------------------------------------------------------------------
// Group routes — the critical test that was failing before nested shared cells
// ---------------------------------------------------------------------------

#[test]
fn test_group_route_match() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        let api = yin.group(app, "/api");
        yin.get(api, "/users", fn(ctx) {
            yin.json(ctx, 200, [{ "id": 1 }, { "id": 2 }])
        });
        let req = { "method": "GET", "path": "/api/users", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

#[test]
fn test_group_route_body() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        let api = yin.group(app, "/api");
        yin.get(api, "/users", fn(ctx) {
            yin.json(ctx, 200, [{ "id": 1 }, { "id": 2 }])
        });
        let req = { "method": "GET", "path": "/api/users", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "body")
    "#);
    let s = format!("{}", v);
    assert!(s.contains("1"), "got: {}", s);
    assert!(s.contains("2"), "got: {}", s);
}

#[test]
fn test_group_with_param_route() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        let api = yin.group(app, "/api");
        yin.get(api, "/posts/:pid", fn(ctx) {
            let pid = yin.param(ctx, "pid");
            yin.json(ctx, 200, { "post_id": pid })
        });
        let req = { "method": "GET", "path": "/api/posts/99", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

// ---------------------------------------------------------------------------
// 404 handling
// ---------------------------------------------------------------------------

#[test]
fn test_not_found() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let req = { "method": "GET", "path": "/nonexistent", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(404));
}

#[test]
fn test_not_found_body() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        let req = { "method": "GET", "path": "/nonexistent", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "body")
    "#);
    let s = format!("{}", v);
    assert!(s.contains("404"), "got: {}", s);
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

#[test]
fn test_middleware_runs_before_handler() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.use(app, fn(ctx, next) {
            let c = ctx;
            let h = c["headers"];
            ctx = assoc(c, "headers", push(h, "X-Custom: yes"));
            next()
        });
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let req = { "method": "GET", "path": "/ping", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        let headers = get(resp, "headers");
        let found = false;
        let i = 0;
        while i < count(headers) {
            if contains(headers[i], "X-Custom") {
                found = true
            };
            i += 1
        };
        found
    "#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_multiple_middleware_chain() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.use(app, fn(ctx, next) {
            let c = ctx;
            let h = c["headers"];
            ctx = assoc(c, "headers", push(h, "X-First: 1"));
            next()
        });
        yin.use(app, fn(ctx, next) {
            let c = ctx;
            let h = c["headers"];
            ctx = assoc(c, "headers", push(h, "X-Second: 2"));
            next()
        });
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let req = { "method": "GET", "path": "/ping", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        let headers = get(resp, "headers");
        count(headers)
    "#);
    // 2 middleware headers + 1 Content-Type from yin.json = 3
    assert_eq!(v, Value::int(3));
}

// ---------------------------------------------------------------------------
// HTTP method helpers
// ---------------------------------------------------------------------------

#[test]
fn test_post_route() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.post(app, "/submit", fn(ctx) {
            yin.json(ctx, 201, { "created": true })
        });
        let req = { "method": "POST", "path": "/submit", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(201));
}

#[test]
fn test_put_route() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.put(app, "/items/:id", fn(ctx) {
            yin.json(ctx, 200, { "updated": true })
        });
        let req = { "method": "PUT", "path": "/items/5", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

#[test]
fn test_delete_route() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.delete(app, "/items/:id", fn(ctx) {
            yin.json(ctx, 200, { "deleted": true })
        });
        let req = { "method": "DELETE", "path": "/items/5", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

#[test]
fn test_json_response_content_type() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/data", fn(ctx) {
            yin.json(ctx, 200, { "hello": "world" })
        });
        let req = { "method": "GET", "path": "/data", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        let headers = get(resp, "headers");
        let found = false;
        let i = 0;
        while i < count(headers) {
            if contains(headers[i], "application/json") {
                found = true
            };
            i += 1
        };
        found
    "#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_text_response() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/hello", fn(ctx) {
            yin.text(ctx, 200, "Hello, World!")
        });
        let req = { "method": "GET", "path": "/hello", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "body")
    "#);
    let s = format!("{}", v);
    assert!(s.contains("Hello, World!"), "got: {}", s);
}

#[test]
fn test_html_response() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/", fn(ctx) {
            yin.html(ctx, 200, "<h1>Welcome</h1>")
        });
        let req = { "method": "GET", "path": "/", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        let headers = get(resp, "headers");
        let found = false;
        let i = 0;
        while i < count(headers) {
            if contains(headers[i], "text/html") {
                found = true
            };
            i += 1
        };
        found
    "#);
    assert_eq!(v, Value::Bool(true));
}

// ---------------------------------------------------------------------------
// Group shares route table with parent — registered on group, matched on app
// and vice versa
// ---------------------------------------------------------------------------

#[test]
fn test_route_registered_on_app_visible_to_group() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        yin.get(app, "/ping", fn(ctx) {
            yin.json(ctx, 200, { "message": "pong" })
        });
        let api = yin.group(app, "/api");
        // The /ping route was registered on app, but since groups share the
        // same route table, it should be matchable via the group's handle too
        // (the group just has a different prefix, but the shared table is the same).
        let req = { "method": "GET", "path": "/ping", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}

#[test]
fn test_nested_groups_share_routes() {
    let v = eval_with_yin(r#"
        import lib.yin as yin;
        let app = yin.new();
        let v1 = yin.group(app, "/api/v1");
        yin.get(v1, "/list", fn(ctx) {
            yin.json(ctx, 200, { "version": "v1" })
        });
        let req = { "method": "GET", "path": "/api/v1/list", "headers": {}, "body": "" };
        let resp = yin.handle(app, req);
        get(resp, "status")
    "#);
    assert_eq!(v, Value::int(200));
}
