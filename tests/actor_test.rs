//! Phase 2 actor runtime tests: spawn / `!` / `?` / reply / state.

use onely::Interpreter;
use onely::Value;

// We use eval_source directly for tests that need the return value, since
// `run` drains mailboxes and returns ().
fn eval_val(src: &str) -> Value {
    let mut interp = Interpreter::new();
    // eval_source does NOT drain mailboxes (it's not `run`), but for `?`
    // (synchronous request) the reply is returned inline, so this is fine
    // for most tests. For `!`-only tests we use `run` via eval_err() or
    // call interp.run() directly.
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

fn eval_err(src: &str) -> String {
    let mut interp = Interpreter::new();
    match interp.run(src) {
        Ok(()) => panic!("expected error, got success"),
        Err(e) => format!("{}", e),
    }
}

// ---------------------------------------------------------------------------
// Basic spawn + request/reply
// ---------------------------------------------------------------------------

#[test]
fn test_spawn_and_request() {
    let src = r#"
        actor Greeter {
            on hello() { reply("hello, world") }
        }
        let g = spawn Greeter();
        g ? hello()
    "#;
    match eval_val(src) {
        Value::Str(s) => assert_eq!(&**s, "hello, world"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_actor_state() {
    let src = r#"
        actor Counter {
            state count = 0
            on inc() { count = count + 1 }
            on get() { reply(count) }
        }
        let c = spawn Counter();
        c ? inc();
        c ? inc();
        c ? inc();
        c ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int 3, got {:?}", other),
    }
}

#[test]
fn test_actor_state_persists_across_requests() {
    let src = r#"
        actor Adder {
            state total = 10
            on add(n) { total = total + n; reply(total) }
        }
        let a = spawn Adder();
        let r1 = a ? add(5);
        let r2 = a ? add(20);
        r2
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "35"),
        other => panic!("expected Int 35, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Fire-and-forget send (`!`)
// ---------------------------------------------------------------------------

#[test]
fn test_fire_and_forget_send() {
    // `!` messages are processed at the end of `run` (drain_mailboxes).
    // We verify the side effect by checking state after the drain.
    let src = r#"
        actor Counter {
            state count = 0
            on inc() { count = count + 1 }
            on get() { reply(count) }
        }
        let c = spawn Counter();
        c ! inc();
        c ! inc();
        // The `!` messages are processed during drain; we can't observe
        // the result directly, but the program should run without error.
    "#;
    // Just verify it runs without error.
    let mut interp = Interpreter::new();
    interp.run(src).expect("should run without error");
}

#[test]
fn test_send_then_request_sees_effect() {
    // `?` is synchronous, so we use it to set state, then check.
    // This verifies that state mutations from `?` are visible to later `?`.
    let src = r#"
        actor Counter {
            state count = 0
            on bump() { count = count + 1 }
            on get() { reply(count) }
        }
        let c = spawn Counter();
        c ? bump();
        c ? bump();
        c ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "2"),
        other => panic!("expected Int 2, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Multiple actors
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_actors_isolated_state() {
    let src = r#"
        actor Counter {
            state count = 0
            on inc() { count = count + 1 }
            on get() { reply(count) }
        }
        let c1 = spawn Counter();
        let c2 = spawn Counter();
        c1 ? inc();
        c1 ? inc();
        c1 ? inc();
        c2 ? inc();
        let n1 = c1 ? get();
        let n2 = c2 ? get();
        n1 * 100 + n2
    "#;
    match eval_val(src) {
        // c1=3, c2=1 → 301
        Value::Int(n) => assert_eq!(n.to_string(), "301"),
        other => panic!("expected Int 301, got {:?}", other),
    }
}

#[test]
fn test_actor_with_args() {
    let src = r#"
        actor Counter {
            state count = 0
            on add(n) { count = count + n; reply(count) }
            on get() { reply(count) }
        }
        let c = spawn Counter();
        c ? add(10);
        c ? add(5);
        c ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "15"),
        other => panic!("expected Int 15, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Actor calling functions from global scope
// ---------------------------------------------------------------------------

#[test]
fn test_actor_calls_global_function() {
    let src = r#"
        fn double(x) { x * 2 }
        actor Worker {
            state result = 0
            on compute(n) { result = double(n); reply(result) }
        }
        let w = spawn Worker();
        w ? compute(21)
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "42"),
        other => panic!("expected Int 42, got {:?}", other),
    }
}

#[test]
fn test_actor_with_fn_def_inside() {
    let src = r#"
        actor Math {
            state acc = 0
            fn sq(x) { x * x }
            on square(n) { acc = sq(n); reply(acc) }
            on get() { reply(acc) }
        }
        let m = spawn Math();
        m ? square(6);
        m ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "36"),
        other => panic!("expected Int 36, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Messages with arguments
// ---------------------------------------------------------------------------

#[test]
fn test_message_with_multiple_args() {
    let src = r#"
        actor Calculator {
            on add(a, b) { reply(a + b) }
            on mul(a, b) { reply(a * b) }
        }
        let calc = spawn Calculator();
        let s = calc ? add(3, 4);
        let p = calc ? mul(5, 6);
        s + p
    "#;
    match eval_val(src) {
        // 7 + 30 = 37
        Value::Int(n) => assert_eq!(n.to_string(), "37"),
        other => panic!("expected Int 37, got {:?}", other),
    }
}

#[test]
fn test_message_with_variant_args() {
    let src = r#"
        actor Box {
            state value = 0
            on set(v) { value = v; reply(nil) }
            on get() { reply(value) }
        }
        let b = spawn Box();
        b ? set(99);
        b ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "99"),
        other => panic!("expected Int 99, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Actor self-reference
// ---------------------------------------------------------------------------

#[test]
fn test_actor_self_reference() {
    let src = r#"
        actor Echo {
            on ping() { reply(self) }
        }
        let e = spawn Echo();
        e ? ping()
    "#;
    match eval_val(src) {
        Value::Actor(_) => {} // self returns the actor ref
        other => panic!("expected Actor, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Reply without explicit reply (falls through)
// ---------------------------------------------------------------------------

#[test]
fn test_handler_without_reply_returns_nil() {
    let src = r#"
        actor Worker {
            on do_work() { 42 }
        }
        let w = spawn Worker();
        w ? do_work()
    "#;
    // Handler body is `42` but no `reply`, so it falls through → nil.
    match eval_val(src) {
        Value::Nil => {}
        other => panic!("expected Nil (no reply), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_send_to_non_actor() {
    let err = eval_err("42 ! hello");
    assert!(err.contains("type error") || err.contains("Actor"));
}

#[test]
fn test_request_to_non_actor() {
    let err = eval_err("42 ? hello");
    assert!(err.contains("type error") || err.contains("Actor"));
}

#[test]
fn test_no_matching_handler() {
    let src = r#"
        actor Quiet {}
        let q = spawn Quiet();
        q ? hello()
    "#;
    let err = eval_err(src);
    assert!(err.contains("no `on hello` handler") || err.contains("handler"));
}

#[test]
fn test_spawn_unknown_actor() {
    let err = eval_err("spawn NoSuchActor()");
    assert!(err.contains("not defined") || err.contains("NoSuchActor"));
}

#[test]
fn test_handler_arity_mismatch() {
    let src = r#"
        actor Calc {
            on add(a, b) { reply(a + b) }
        }
        let c = spawn Calc();
        c ? add(1)
    "#;
    let err = eval_err(src);
    assert!(err.contains("expects") || err.contains("argument"));
}

// ---------------------------------------------------------------------------
// Actor with pattern-matched messages
// ---------------------------------------------------------------------------

#[test]
fn test_actor_message_dispatch_by_name() {
    let src = r#"
        actor Dispatcher {
            state log = ""
            on info(msg) { log = msg; reply(log) }
            on error(msg) { log = "ERR: " + msg; reply(log) }
        }
        let d = spawn Dispatcher();
        let r1 = d ? info("hello");
        let r2 = d ? error("boom");
        r2
    "#;
    match eval_val(src) {
        Value::Str(s) => assert_eq!(&**s, "ERR: boom"),
        other => panic!("expected Str, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Complex: actor as state machine
// ---------------------------------------------------------------------------

#[test]
fn test_actor_state_machine() {
    let src = r#"
        actor TrafficLight {
            state color = "red"
            on cycle() {
                color = match color {
                    "red" => "green",
                    "green" => "yellow",
                    "yellow" => "red",
                    _ => "red"
                };
                reply(color)
            }
            on current() { reply(color) }
        }
        let tl = spawn TrafficLight();
        let c1 = tl ? cycle();
        let c2 = tl ? cycle();
        let c3 = tl ? cycle();
        let c4 = tl ? cycle();
        // red → green → yellow → red → green
        c1 + "-" + c2 + "-" + c3 + "-" + c4
    "#;
    match eval_val(src) {
        Value::Str(s) => assert_eq!(&**s, "green-yellow-red-green"),
        other => panic!("expected Str, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Regression: handler name must not shadow global builtins.
// An `on get()` handler should not prevent calling the builtin `get(map, key)`
// from within any handler body.
// ---------------------------------------------------------------------------

#[test]
fn test_handler_name_does_not_shadow_builtin() {
    let src = r#"
        actor KVStore {
            state data = dissoc({ __init: 0 }, "__init")
            on put(key, val) { data = assoc(data, key, val); reply(nil) }
            on get(key) { reply(get(data, key)) }
            on size() { reply(count(data)) }
        }
        let s = spawn KVStore();
        s ? put("a", 1);
        s ? put("b", 2);
        let va = s ? get("a");
        let vb = s ? get("b");
        let sz = s ? size();
        va + vb * 10 + sz * 100
    "#;
    match eval_val(src) {
        // 1 + 20 + 200 = 221
        Value::Int(n) => assert_eq!(n.to_string(), "221"),
        other => panic!("expected Int 221, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// yield — inline mailbox draining (Phase 4.6)
// ---------------------------------------------------------------------------

#[test]
fn test_yield_drains_fire_and_forget_messages() {
    // `yield` should process `!` messages that were queued before it,
    // making their side effects visible to subsequent `?` requests.
    let src = r#"
        actor Counter {
            state count = 0
            on inc() { count = count + 1 }
            on get() { reply(count) }
        }
        let c = spawn Counter();
        c ! inc();
        c ! inc();
        c ! inc();
        // Without yield, these `!` messages would only be drained at
        // program exit. With yield, they are processed here.
        yield;
        c ? get()
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "3", "yield should drain 3 inc messages"),
        other => panic!("expected Int 3, got {:?}", other),
    }
}

#[test]
fn test_yield_returns_nil() {
    let src = r#"
        actor A { on ping() { reply(nil) } }
        let a = spawn A();
        a ! ping();
        yield
    "#;
    match eval_val(src) {
        Value::Nil => {}
        other => panic!("yield should return Nil, got {:?}", other),
    }
}

#[test]
fn test_yield_with_no_actors_is_noop() {
    // yield with no live actors should just return Nil.
    assert_eq!(eval_val("yield"), Value::Nil);
}

#[test]
fn test_yield_processes_multiple_actors() {
    // yield should drain mailboxes of ALL live actors, not just one.
    let src = r#"
        actor Box {
            state val = 0
            on set(v) { val = v }
            on get() { reply(val) }
        }
        let a = spawn Box();
        let b = spawn Box();
        a ! set(10);
        b ! set(20);
        yield;
        let va = a ? get();
        let vb = b ? get();
        va + vb
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "30"),
        other => panic!("expected Int 30, got {:?}", other),
    }
}

#[test]
fn test_yield_in_a_loop_for_event_processing() {
    // Simulate an event loop: queue messages, yield to process them,
    // repeat. This is the pattern used by the HTTP server.
    let src = r#"
        actor Sum {
            state total = 0
            on add(n) { total = total + n }
            on total() { reply(total) }
        }
        let s = spawn Sum();
        let i = 0;
        while i < 5 {
            s ! add(i);
            yield;
            i = i + 1
        };
        s ? total()
    "#;
    // 0 + 1 + 2 + 3 + 4 = 10
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "10"),
        other => panic!("expected Int 10, got {:?}", other),
    }
}
