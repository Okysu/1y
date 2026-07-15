---
title: Transactional Counter
---

# Transactional Counter

A counter is the smallest "shared mutable state" problem in the concurrency world: several execution flows each want to add one to the same number, and if synchronization is mishandled, updates get lost. Threads-plus-locks solve it but force you to worry about deadlocks and lock granularity; Actors solve it but require a message for every read and write, and atomic updates across several variables are awkward. 1y offers a third path — **Software Transactional Memory (STM)**. This example builds a thread-safe counter with `shared` and `transact`, unfolding snapshot isolation, conflict retry, and nested transactions, then compares it with the Actor approach.

## shared: A Sharable Transactional Cell

An ordinary `let` binding in 1y is immutable. To express "a piece of state that may be modified concurrently," you declare a transactional cell with `shared`:

```1y
shared counter = 0;
println(counter);      // 0
```

`shared counter = 0` creates a versioned mutable cell initialized to `0`. Reading the name `counter` auto-dereferences it and returns its current value; assigning to it outside a transaction (e.g. `counter = 10`) writes through directly and bumps its version. In concurrent settings, however, we almost always put reads and writes inside a transaction to gain atomicity.

## Atomic Increment with transact

The most basic need is that "read—modify—write" be atomic: read the old value, add one, write the new value — these three steps must either all succeed or all roll back. `transact` exists for exactly this:

```1y
shared counter = 0;

fn bump() -> Int {
    transact {
        counter = counter + 1;
        counter
    }
}

println(bump());    // 1
println(bump());    // 2
println(bump());    // 3
```

Let us unpack what happens inside `bump`:

- `transact { ... }` opens a transaction. Writes to `shared` cells inside the body do not take effect immediately; they are first buffered in a "write set."
- `counter = counter + 1` reads the current value of `counter` (inside a transaction, this read sees a consistent snapshot from when the transaction began), adds one, and records the new value in the write set.
- The final `counter` is the tail expression of the transaction body, and its value becomes the return value of the whole `transact`. On commit, all changes in the write set are written atomically at once, and the version numbers of the affected cells are bumped.

The key point: **on commit, if any cell the transaction read has been changed by another transaction, the whole transaction retries automatically.** This is STM's optimistic concurrency — no locks, just re-run on conflict. For something like a counter, where conflicts are rare, the retry cost is essentially negligible.

## Snapshot Isolation: Read Your Own Writes

Inside a transaction you can see your own just-written values — this is "read-your-writes," part of snapshot isolation:

```1y
shared x = 0;
let snap = transact {
    x = x + 1;       // buffered write: x = 1
    x = x + 1;       // reading x again sees the buffered 1, adds one to get 2
    x
};
println(snap);       // 2
println(x);          // 2 — visible to the outside after commit
```

On the second read of `x`, the transaction returns the buffered value `1` from the write set, not the committed `0`. This lets the code inside a transaction read as though it were single-threaded and sequential — you never have to mentally track "which values are committed and which are buffered"; the transaction does that for you.

## retry: Wait Until a Condition Holds

`retry` abandons the current transaction and starts it over, until it can commit without conflict. Combined with the rule that "only `shared` writes are discarded on retry, while `let` variables survive across retries," it elegantly implements "wait for a condition":

```1y
let attempts = 0;
shared ready = 0;

let result = transact {
    attempts = attempts + 1;
    if attempts < 3 { retry };
    ready = 99;
    ready + attempts
};
println(result);          // 102 — ready (99) + attempts (3)
println(attempts);        // 3 — the let variable survived the retries
```

Here `attempts < 3` triggers `retry`, re-running the transaction from the top. Because `attempts` is an ordinary `let` variable, it is not rolled back, so each retry increments it — until the third pass, when the condition no longer holds and the transaction can finally commit. This is a very clean "retry until success" pattern, often used to wait for some shared condition to be satisfied by another flow. STM retries up to 64 times by default and then raises an exception, preventing an infinite loop.

## Nested Transactions

`transact` can be nested. The inner transaction's write set **merges into the outer** one rather than committing independently; an exception raised inside only rolls back the inner writes, leaving the outer untouched:

```1y
shared m = 0;
transact {
    m = m + 1;                          // outer: m = 1
    try {
        transact {
            m = m + 10;                 // inner: m = 11
            raise "inner fail"          // inner rolls back
        }
    } rescue as _e {
        nil
    };
    m                                   // outer still sees m = 1
};
println(m);                             // 1
```

The inner transaction fails because of `raise`, so its `m = m + 10` is discarded; the outer transaction continues and ultimately commits only `m = m + 1`. This "partial rollback plus global commit" semantics let you safely wrap a piece of fallible logic in an inner transaction without jeopardizing the outer transaction's overall atomicity.

## Comparison with the Actor Approach

The same counter, written with an Actor, looks like this:

```1y
actor Counter {
    state count = 0
    on inc() { count = count + 1; reply(count) }
    on get() { reply(count) }
}

let c = spawn Counter();
c ? inc();
```

Each approach has its own emphasis:

| Dimension | Actor counter | STM counter |
|-----------|--------------|-------------|
| State encapsulation | Fully private, messages only | `shared` cells can be read/written directly by many parties |
| Atomic multi-variable updates | Needs a protocol, awkward | `transact` supports it natively, composable |
| Invocation | `?` blocking request/reply | Plain function call, synchronous return |
| Failure handling | Lost messages / timeouts need design | Automatic retry, clear semantics |
| Best for | Long-lived stateful services | Coordinated shared access across boundaries |

The rule of thumb matches the previous example: **default to Actors, reach for STM only when several shared cells must change atomically together.** A counter looks simple, but the moment it has to change atomically alongside other state (say, "increment the counter AND append a record to a log"), STM's composability advantage shows immediately — you just put both writes inside one `transact`, with no need to redesign lock ordering or a message protocol.
