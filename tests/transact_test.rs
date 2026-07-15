//! Phase 3 transactional memory tests: `shared` / `transact` / `retry`.
//!
//! Covers: basic shared ref read/write, transactional atomicity (rollback on
//! exception), snapshot isolation, nested transactions, retry semantics,
//! versioning, interaction with actors, and a high-conflict stress test.

use onely::Interpreter;
use onely::Value;

fn eval_val(src: &str) -> Value {
    let mut interp = Interpreter::new();
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

// Tree-walking interpreters map `1y` recursion to native Rust recursion, so
// the stress tests (which recurse ~100 deep via `loop_n`) overflow the default
// test-thread stack on Windows. This helper runs on a worker thread with a
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
// Basic shared ref read/write (no transaction)
// ---------------------------------------------------------------------------

#[test]
fn test_shared_basic_read() {
    let src = r#"
        shared x = 42
        x
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "42"),
        other => panic!("expected Int 42, got {:?}", other),
    }
}

#[test]
fn test_shared_basic_write_outside_transaction() {
    let src = r#"
        shared x = 10
        x = 99
        x
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "99"),
        other => panic!("expected Int 99, got {:?}", other),
    }
}

#[test]
fn test_shared_with_collection() {
    let src = r#"
        shared items = [1, 2, 3]
        items = push(items, 4)
        count(items)
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "4"),
        other => panic!("expected Int 4, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Transactional atomicity
// ---------------------------------------------------------------------------

#[test]
fn test_transact_basic_write() {
    let src = r#"
        shared counter = 0
        transact {
            counter = counter + 1
        }
        counter
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "1"),
        other => panic!("expected Int 1, got {:?}", other),
    }
}

#[test]
fn test_transact_multiple_writes() {
    let src = r#"
        shared a = 0
        shared b = 0
        transact {
            a = a + 10;
            b = b + 20;
            a = a + 1
        }
        a + b
    "#;
    match eval_val(src) {
        // a = 11, b = 20 → 31
        Value::Int(n) => assert_eq!(n.to_string(), "31"),
        other => panic!("expected Int 31, got {:?}", other),
    }
}

#[test]
fn test_transact_returns_body_value() {
    let src = r#"
        shared x = 1
        let r = transact {
            x = x + 10;
            x * 2
        }
        r
    "#;
    match eval_val(src) {
        // x = 11, returns 22
        Value::Int(n) => assert_eq!(n.to_string(), "22"),
        other => panic!("expected Int 22, got {:?}", other),
    }
}

#[test]
fn test_transact_rollback_on_exception() {
    let src = r#"
        shared x = 100
        let r = try {
            transact {
                x = x + 50;
                raise "oops"
            }
        } rescue as e {
            "caught"
        }
        let after = x
        r + "-" + str(after)
    "#;
    match eval_val(src) {
        // Transaction rolled back: x should still be 100
        Value::Str(s) => assert_eq!(&**s, "caught-100"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_transact_rollback_partial_writes() {
    let src = r#"
        shared a = 0
        shared b = 0
        try {
            transact {
                a = 1;
                b = 2;
                raise "fail"
            }
        } rescue as _e {
            nil
        }
        a * 10 + b
    "#;
    match eval_val(src) {
        // Both writes rolled back → 0
        Value::Int(n) => assert_eq!(n.to_string(), "0"),
        other => panic!("expected Int 0, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Snapshot isolation
// ---------------------------------------------------------------------------

#[test]
fn test_transact_snapshot_isolation() {
    let src = r#"
        shared x = 0
        transact {
            x = x + 1;
            // Reading x again should see the transaction's buffered write (1),
            // not the committed value (0).
            x = x + 1;
            x
        }
    "#;
    match eval_val(src) {
        // x = 0 → 1 → 2
        Value::Int(n) => assert_eq!(n.to_string(), "2"),
        other => panic!("expected Int 2, got {:?}", other),
    }
}

#[test]
fn test_transact_read_outside_sees_committed() {
    let src = r#"
        shared x = 5
        let before = transact {
            x = x + 10;
            x
        }
        // Outside the transaction, x should be 15 (committed)
        let after = x
        before + after
    "#;
    match eval_val(src) {
        // before=15, after=15 → 30
        Value::Int(n) => assert_eq!(n.to_string(), "30"),
        other => panic!("expected Int 30, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Nested transactions
// ---------------------------------------------------------------------------

#[test]
fn test_nested_transact_commit_merges_to_parent() {
    let src = r#"
        shared x = 0
        transact {
            x = x + 1;
            transact {
                x = x + 1
            };
            x
        }
    "#;
    match eval_val(src) {
        // Inner commit merges to parent: x = 0 → 1 → 2
        Value::Int(n) => assert_eq!(n.to_string(), "2"),
        other => panic!("expected Int 2, got {:?}", other),
    }
}

#[test]
fn test_nested_transact_rollback_inner_only() {
    let src = r#"
        shared x = 0
        transact {
            x = x + 1;          // x = 1 (in parent write-set)
            try {
                transact {
                    x = x + 10;
                    raise "inner fail"
                }
            } rescue as _e {
                nil
            };
            // Inner rolled back, parent's x should still be 1
            x
        }
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "1"),
        other => panic!("expected Int 1, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Retry
// ---------------------------------------------------------------------------

#[test]
fn test_retry_basic() {
    // A retry that unconditionally loops should eventually hit the limit.
    let err = eval_err(r#"
        shared x = 0
        transact {
            retry
        }
    "#);
    assert!(
        err.contains("retries") || err.contains("retry"),
        "expected retry exhaustion, got: {}",
        err
    );
}

#[test]
fn test_retry_conditional_then_succeeds() {
    // Use a non-shared `let` variable to track retry attempts.
    // `let` variables are NOT buffered by the transaction — they persist
    // across retries. Only `shared` refs are buffered and discarded on retry.
    let src = r#"
        let attempts = 0
        shared x = 0
        transact {
            attempts = attempts + 1;
            if attempts < 3 { retry };
            x = 42;
            x + attempts
        }
    "#;
    match eval_val(src) {
        // attempt 1: attempts=1, 1<3 → retry (shared writes discarded)
        // attempt 2: attempts=2, 2<3 → retry
        // attempt 3: attempts=3, 3<3 false → x=42, returns 42+3=45
        Value::Int(n) => assert_eq!(n.to_string(), "45"),
        other => panic!("expected Int 45, got {:?}", other),
    }
}

#[test]
fn test_retry_discards_shared_writes() {
    // Verify that `retry` discards buffered writes to shared refs, but
    // non-shared `let` bindings persist.
    let src = r#"
        let attempts = 0
        shared x = 10
        transact {
            attempts = attempts + 1;
            x = x + 100;    // buffered: x = 110
            if attempts < 2 { retry };
            // After retry, x's write-set is discarded → x reads as 10 again.
            // On 2nd attempt: x = 10 + 100 = 110, no retry.
            x + attempts
        }
    "#;
    match eval_val(src) {
        // attempt 2: x = 10 + 100 = 110, returns 110 + 2 = 112
        Value::Int(n) => assert_eq!(n.to_string(), "112"),
        other => panic!("expected Int 112, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

#[test]
fn test_shared_version_increments_on_direct_write() {
    // Each direct write bumps the version.
    let src = r#"
        shared x = 0
        x = 1
        x = 2
        x = 3
        x
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int 3, got {:?}", other),
    }
}

#[test]
fn test_transact_commit_bumps_version() {
    let src = r#"
        shared x = 0
        transact { x = x + 1 }
        transact { x = x + 1 }
        transact { x = x + 1 }
        x
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int 3, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Field/index assignment on shared refs
// ---------------------------------------------------------------------------

#[test]
fn test_shared_field_assignment() {
    let src = r#"
        type Point = { x: Int, y: Int }
        shared p = Point({ x: 0, y: 0 })
        transact {
            p.x = 10;
            p.y = 20
        }
        p.x + p.y
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "30"),
        other => panic!("expected Int 30, got {:?}", other),
    }
}

#[test]
fn test_shared_index_assignment() {
    let src = r#"
        shared vec = [1, 2, 3]
        transact {
            vec[0] = 99
        }
        vec[0]
    "#;
    match eval_val(src) {
        Value::Int(n) => assert_eq!(n.to_string(), "99"),
        other => panic!("expected Int 99, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Interaction with actors
// ---------------------------------------------------------------------------

#[test]
fn test_actor_accesses_shared_state() {
    let src = r#"
        shared counter = 0
        actor Worker {
            on bump() {
                transact {
                    counter = counter + 1;
                    reply(counter)
                }
            }
            on get() { reply(counter) }
        }
        let w1 = spawn Worker();
        let w2 = spawn Worker();
        let a = w1 ? bump();
        let b = w2 ? bump();
        a + b
    "#;
    match eval_val(src) {
        // a=1, b=2 → 3
        Value::Int(n) => assert_eq!(n.to_string(), "3"),
        other => panic!("expected Int 3, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_retry_outside_transact_is_error() {
    let err = eval_err("retry");
    assert!(
        err.contains("retry"),
        "expected retry error, got: {}",
        err
    );
}

// ---------------------------------------------------------------------------
// Stress test: many sequential transactions
// ---------------------------------------------------------------------------

#[test]
fn test_stress_sequential_transactions() {
    // Simulate 100 bank transfers between two accounts, each in a transaction.
    // After 100 transfers of $1 each, total should be preserved.
    let src = r#"
        shared account_a = 1000
        shared account_b = 0

        fn transfer(amount) {
            transact {
                account_a = account_a - amount;
                account_b = account_b + amount
            }
        }

        fn loop_n(n) {
            if n > 0 {
                transfer(1);
                loop_n(n - 1)
            }
        }

        loop_n(100)
        account_a + account_b
    "#;
    // Total preserved: 1000 + 0 = 1000
    assert_eq!(eval_big(src), "1000");
}

#[test]
fn test_stress_transfer_with_rollback() {
    // Every 3rd transfer fails (raises), but the total should still be
    // preserved because failed transactions roll back.
    let src = r#"
        shared a = 100
        shared b = 0
        shared i = 0

        fn try_transfer(amount) {
            try {
                transact {
                    i = i + 1;
                    if i % 3 == 0 { raise "fail" };
                    a = a - amount;
                    b = b + amount
                }
            } rescue as _e {
                nil
            }
        }

        fn loop_n(n) {
            if n > 0 {
                try_transfer(1);
                loop_n(n - 1)
            }
        }

        loop_n(10)
        // 10 transfers attempted, but every 3rd (i=3,6,9) fails.
        // Successful: i=1,2,4,5,7,8,10 → 7 transfers
        a + b
    "#;
    // Total preserved: 100 + 0 = 100 (7 transfers of $1 each)
    assert_eq!(eval_big(src), "100");
}

// ---------------------------------------------------------------------------
// COW: shared collections use structural sharing
// ---------------------------------------------------------------------------

#[test]
fn test_shared_cow_collection() {
    // Modifying a shared vector inside a transaction creates a new vector
    // (via `push`), but the old value is preserved if the transaction
    // rolls back.
    let src = r#"
        shared data = [1, 2, 3]
        let old_count = count(data)
        try {
            transact {
                data = push(data, 4);
                data = push(data, 5);
                raise "rollback"
            }
        } rescue as _e {
            nil
        }
        let new_count = count(data)
        old_count * 10 + new_count
    "#;
    match eval_val(src) {
        // old=3, new=3 (rolled back)
        Value::Int(n) => assert_eq!(n.to_string(), "33"),
        other => panic!("expected Int 33, got {:?}", other),
    }
}
