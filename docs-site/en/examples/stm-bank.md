---
title: STM Bank Transfers
---

# STM Bank Transfers

Multi-account concurrent transfer is a classic scenario in concurrent programming: several accounts must debit and credit each other, and along the way a balance may be insufficient or a condition may need to be waited on. The traditional solution requires locking each account and carefully agreeing on a global lock order to avoid deadlocks — once the number of accounts grows, the combinatorial complexity of locks explodes. 1y's Software Transactional Memory (STM) offers a lock-free answer: `shared` declares shareable accounts, `transact` packages multiple reads and writes into an atomic operation, `retry` elegantly waits for a balance to arrive, and multiple Actors can call the same transfer function concurrently without stepping on each other. This example walks through the whole flow, from account modeling to invariant verification.

## Shared Accounts

Each account is a `shared` cell holding an integer balance. A `shared` cell is a versioned mutable reference — it can be read directly outside a transaction and atomically modified inside one:

```1y
shared acct_alice = 1000;
shared acct_bob = 500;
shared acct_carol = 300;
shared total_transfers = 0;   // metrics: number of successful transfers

fn total_money() -> Int {
    transact {
        acct_alice + acct_bob + acct_carol
    }
}

println("initial total: " + str(total_money()));   // 1800
```

The three accounts sum to 1800, which is the invariant we will preserve later. `total_transfers` is a metric counter tracking the number of successful transfers. Note that `total_money` wraps its reads in a `transact` — so the three reads observe a consistent snapshot taken at the same instant, rather than "reading Alice's old value and then Bob's new value" in some torn state.

To make the transfer function convenient to operate on accounts by name, we prepare two helpers. They access the `shared` cells directly, but inside a `transact`, reads see a snapshot and writes are buffered until commit:

```1y
fn read_account(name) -> Int {
    match name {
        "alice" => acct_alice,
        "bob" => acct_bob,
        "carol" => acct_carol,
        _ => 0
    }
}

fn write_account(name, balance) {
    match name {
        "alice" => acct_alice = balance,
        "bob" => acct_bob = balance,
        "carol" => acct_carol = balance,
        _ => nil
    }
}
```

## Atomic Transfer

The core transfer function wraps "read source, read destination, check balance, write both, bump counter" inside a `transact`. If any step fails, the whole batch rolls back; only when everything succeeds and no other transaction interfered does it commit atomically in one shot:

```1y
fn transfer(src_name, dst_name, amount) -> Bool {
    let attempts = 0;
    transact {
        attempts = attempts + 1;
        if attempts > 3 { false } else {
            let src_balance = read_account(src_name);
            let dst_balance = read_account(dst_name);

            if src_balance < amount {
                retry   // wait for funds — restart the transaction
            };

            // Write both — committed atomically.
            write_account(src_name, src_balance - amount);
            write_account(dst_name, dst_balance + amount);
            total_transfers = total_transfers + 1;
            true
        }
    }
}
```

The tail expression of `transact { ... }` is a boolean indicating whether this transfer succeeded. Two key design choices stand out: **reads and writes are both inside the transaction**, so "check balance — debit — credit" cannot be interleaved with another concurrent transaction's intermediate state; **both `write_account` calls are merely buffered until commit**, then written atomically at once — either both take effect or neither does. There is never an externally visible intermediate state where money was debited but never credited.

## Retry for Balance

What happens when the source account has insufficient funds? Returning `false` immediately is one option, but STM gives a more elegant tool — `retry`. `retry` abandons the current transaction and re-runs it from the top, until it can commit without conflict:

```1y
if src_balance < amount {
    retry   // wait for funds — restart the transaction
};
```

There is a pitfall: if the balance never becomes sufficient, the transaction will retry forever. 1y resolves this with one rule — **only `shared` writes are discarded on retry; ordinary `let` variables survive across retries**. So we use a `let attempts = 0` counter that increments on every retry and gives up after 3 attempts, returning `false`:

```1y
let attempts = 0;
transact {
    attempts = attempts + 1;
    if attempts > 3 { false } else {
        // ... actual transfer logic ...
    }
}
```

`attempts` is not rolled back, so it faithfully records the number of retries. This is 1y STM's "wait for a condition" pattern: use `retry` to wait until some shared condition is satisfied by another flow, while a `let` variable bounds the retries to prevent an infinite loop. STM retries up to 64 times by default; here we use a tighter limit of 3 so that "insufficient funds" fails fast.

## Actor + STM

The transfer logic is an ordinary `transact` function that can be called by multiple Actors at once. We define a `Teller` actor that receives transfer requests and calls `transfer` internally:

```1y
actor Teller {
    on transfer(src, dst, amount) {
        let ok = transfer(src, dst, amount);
        reply(ok)
    }
    on balance(name) {
        reply(read_account(name))
    }
}

let teller = spawn Teller();
```

Multiple `Teller`s can run in parallel, each handling its own users' transfer requests. STM guarantees that their modifications to the `shared` accounts do not interfere: if two transactions conflict, the runtime automatically retries one of them — no locks anywhere in the code:

```1y
// Alice sends 100 to Bob.
let r1 = teller ? transfer("alice", "bob", 100);
println("alice -> bob 100: " + str(r1));              // true

// Bob sends 50 to Carol.
let r2 = teller ? transfer("bob", "carol", 50);
println("bob -> carol 50: " + str(r2));               // true

// Carol tries to send 99999 (insufficient funds — gives up after retries).
let r3 = teller ? transfer("carol", "alice", 99999);
println("carol -> alice 99999: " + str(r3));          // false
```

The third transfer fails because Carol has insufficient funds, returning `false` after 3 retries. Note how the code reads like single-threaded sequential logic — `teller ? transfer(...)` synchronously waits for the result — while underneath multiple requests can run concurrently, with account integrity guarded by STM.

## Rollback Demo

The atomicity of a transaction protects not just account balances but also writes to any `shared` cell made inside the transaction — including collections like `audit_log`. The `audited_transfer` below debits and credits the accounts while appending a record to the log; if the amount is too large and `raise` fires, all of the changes (balances, log append, counter) roll back together:

```1y
shared audit_log = [];

fn audited_transfer(src, dst, amount) -> Bool {
    try {
        transact {
            let s = read_account(src);
            let d = read_account(dst);
            write_account(src, s - amount);
            write_account(dst, d + amount);
            audit_log = push(audit_log, src + "->" + dst + " " + str(amount));
            if amount > 400 { raise "blocked: amount too large" };
            total_transfers = total_transfers + 1;
            true
        }
    } rescue as _e {
        false
    }
}

let r4 = audited_transfer("alice", "carol", 500);
println("audited alice -> carol 500: " + str(r4));    // false (blocked)
println("audit_log entries: " + str(count(audit_log)));  // 0 — rolled back
```

Several things happen here:

- The transaction first debits Alice, credits Carol, and appends a record to `audit_log` — but these are all **buffered writes**, not yet committed.
- `if amount > 400 { raise ... }` throws an exception, and the entire `transact` write set is discarded.
- `try`/`rescue` catches the exception, and the function returns `false`.
- In the end `count(audit_log)` is `0` — the `push` was also rolled back.

This demonstrates the COW (Copy-on-Write) semantics of `shared` collections: `push(audit_log, ...)` inside a transaction does not mutate the list in place but returns a new list version, which only becomes visible when the transaction commits. On rollback, the new version is simply discarded, and the log remains untouched.

## Invariant Verification

After the whole example runs, we inspect the final state:

```1y
println("");
println("=== Final state ===");
println("alice:  " + str(acct_alice));     // 1000 - 100 = 900
println("bob:    " + str(acct_bob));       // 500 + 100 - 50 = 550
println("carol:  " + str(acct_carol));     // 300 + 50 = 350
println("total:  " + str(total_money()));  // 1800 (conserved)
println("successful transfers: " + str(total_transfers));  // 2
```

The key invariant is **conservation of total money**: no matter how many successful transfers happen, the sum of all balances stays at 1800. The two successful transfers (Alice→Bob 100, Bob→Carol 50) merely move money between accounts, leaving the total unchanged; the third (Carol→Alice) failed due to insufficient funds and touched nothing; the fourth was blocked by `raise` and rolled back, touching nothing as well. In the end `total_money()` is still 1800 — exactly the guarantee that STM's atomicity gives: each transaction either fully takes effect or fully rolls back, so there is never a torn state where money was debited without being credited.

## Compared to Locks

Writing the same bank transfer with traditional locks requires a lock per account and a carefully agreed-upon global lock order to avoid deadlocks. Compared against that, the STM approach looks like this:

| Dimension | Traditional locks | 1y STM |
|-----------|--------------------|--------|
| Deadlocks | Must carefully agree on a global lock order, or two transfers each waiting for the other's lock deadlock | No locks at all; the runtime detects conflicts and retries automatically, deadlock-free |
| Composability | Adding a new involved account means redesigning the lock order, poor composability | Just add the new account to the same `transact`; transactions compose freely |
| Readability | Code is interrupted by lock acquire/release, tangled with business logic | Code reads like single-threaded sequential logic, locks are invisible |
| Failure handling | Deadlock/timeout must be handled manually, state must be restored by hand | Conflicts retry automatically, failures roll back to the pre-transaction state |
| Insufficient funds | Needs condition variables, `wait`/`notify`, easy to get wrong | One `retry` does it, with the semantics of "retry until the condition holds" |
| Performance | Coarse locks limit concurrency, fine locks invite deadlocks | Optimistic concurrency, near-zero overhead when conflicts are rare |

The core insight: **no locks, no lock ordering, no deadlocks**. If two transactions conflict, the STM runtime automatically retries one of them. The code reads like single-threaded sequential logic, yet is safe under concurrency. That is the value of STM — it reduces "safe concurrency" from a hard problem requiring carefully designed lock protocols to the straightforward act of "put the related writes inside one `transact`."
