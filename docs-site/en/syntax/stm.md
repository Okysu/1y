---
title: Software Transactional Memory
---

# Software Transactional Memory

The Actor model banishes sharing through isolation, but some situations genuinely require **sharing** — multiple components reading and writing the same configuration, atomic updates across several data structures, or a buffer shared between producers and consumers. For these cases 1y provides **Software Transactional Memory (STM)**: it replaces the hard-to-reason-about concept of "locks" with "transactions," a concept programmers already know well from databases.

This page covers the complete syntax of STM: `shared` to create transactional cells, direct read/write to access them, `transact` to open a transaction, `retry` to retry, plus nested transactions and the rules for accessing cells outside a transaction.

## Creating a Transactional Cell: shared

`shared expr` creates a **transactional cell** whose value can be read and written within transactions. `shared` returns a cell handle, not the value itself.

```1y
let counter = shared 0;
let name = shared "anon";
let config = shared { host: "localhost", port: 8080 };
```

`shared` accepts any 1y value as the initial value. The cell itself is a **shared reference** — multiple transactions can read and write it concurrently, but every access is protected by STM's isolation and atomicity. You can think of a cell as "a replaceable label": the value the label points to is immutable, but the label itself can be swapped to point at a new value.

## Reading and Writing Cells

Reads and writes to a cell use the **same syntax as ordinary variables** — no special prefix operator is needed. The binding name refers to the cell; reading it yields the current value, assigning to it swaps the value:

- `counter` reads the cell's current value.
- `counter = expr` sets the cell's value to the result of `expr`.

```1y
let v = counter;       // read
counter = v + 1;       // write
```

The meaning depends on context: inside a transaction, reads and writes go through snapshot isolation; outside a transaction, they take effect directly (see [Access Outside a Transaction](#access-outside-a-transaction)). This uniform syntax lets you reuse the same read/write logic in and out of transactions, deciding only whether to wrap it in `transact`.

## Transactions: transact

`transact { ... }` opens a transaction, bringing all reads and writes to cells inside it under **snapshot isolation** and **atomic commit**.

```1y
let counter = shared 0;
let result = transact {
    let v = counter + 1;
    counter = v;
    v           // the transaction's return value
};
```

Key properties of a transaction:

- **Snapshot isolation**: when the transaction begins it takes a snapshot of the cells it will read; throughout the transaction it sees only that consistent snapshot, unpolluted by other transactions' intermediate states.
- **Atomic commit**: at the end, if none of the cells the transaction read have been changed by other transactions, all its writes commit at once; otherwise it rolls back entirely and retries.
- **Has a return value**: the value of the last expression in the `transact` block is the transaction's result. After a successful commit, the caller receives this value.

## Snapshot Isolation

Snapshot isolation means a transaction **sees a consistent state as of the moment it started**, even if other transactions commit changes in the meantime. This lets the logic inside a transaction read as naturally as sequential code:

```1y
// Atomic transfer: either both balances update, or neither does
let alice = shared 100;
let bob = shared 50;

transact {
    let a = alice;
    let b = bob;
    if a >= 30 {
        alice = a - 30;
        bob = b + 30;
    }
};
```

No matter how many transactions run concurrently, a transfer can never end up in the intermediate state "money debited from alice but never credited to bob." Each transaction sees either the pre-transfer snapshot or the post-transfer snapshot — never a half-applied state. This aligns in spirit with the `SERIALIZABLE` / `REPEATABLE READ` isolation levels of databases, except the objects are in-memory cells rather than table rows.

## Commit and Rollback

The transaction's commit protocol is **optimistic concurrency control**:

1. During execution the transaction takes no locks; it reads and writes only on its private snapshot.
2. At commit time, STM checks every cell the transaction read — whether each cell's current value still matches the snapshot.
3. If all match, the commit succeeds and all the transaction's writes take effect atomically.
4. If any cell has been changed by another transaction, the commit fails; the transaction **rolls back entirely** and retries from the beginning.

This "do first, check later" strategy is efficient when conflicts are infrequent: the vast majority of transactions commit on the first try with no locking overhead. Only under high contention do retries pile up. Combined with 1y's immutable data, rollback is nearly free — the snapshot is itself an immutable value, so retrying just discards it, with no in-place mutations to "undo."

## Retry: retry

`retry` explicitly abandons the current transaction and reruns it from the top. It is typically used for the "condition not yet met, try again later" pattern: the transaction finds a precondition not currently satisfied and proactively `retry`s, waiting until the relevant cells change before trying once more.

```1y
// Wait until the balance is sufficient, then debit
transact {
    if alice >= amount {
        alice = alice - amount;
    } else {
        retry        // not enough balance; retry the transaction
    }
};
```

Key points:

- `retry` discards all of the current attempt's writes and resumes execution at the start of the `transact` block.
- To prevent infinite loops, the **maximum number of retries is 64**. If the transaction still cannot commit after that cap, it ultimately fails with an error.
- `retry` differs from automatic rollback: rollback is triggered by conflicts, while `retry` is triggered deliberately by the programmer. Both ultimately follow the same "re-execute" path.

## Nested Transactions

1y supports nested transactions: opening a `transact` inside another `transact`. **When the inner transaction commits, its writes commit to the outer transaction's private snapshot, not to the global state**; only when the outermost transaction commits successfully do all writes become truly visible to the outside.

```1y
transact {
    counter = counter + 1;
    transact {
        // the inner commit goes to the outer snapshot;
        // if the outer rolls back, this rolls back too
        counter = counter + 10;
    };
    // what's read here is the outer snapshot, already including the inner writes
    counter
};
```

Nested transaction semantics:

- A rollback of the inner transaction affects only the inner; the outer is unaffected (as if the inner never happened).
- A commit of the inner transaction does not take effect globally; it merges into the outer. If the outer rolls back, the inner's writes roll back along with it.
- This matches the savepoint semantics of databases: the inner is an atomic sub-unit of the outer.

Nested transactions make "composition" natural: you can split a complex transaction into several smaller ones, each atomic on its own, and let an outer transaction glue them into one larger atomic unit — without hand-writing compensation logic.

## Access Outside a Transaction

Outside a `transact` block, reading and writing a cell are still available, with different semantics:

- **Read outside a transaction** `counter`: reads the cell's current value directly — no snapshot, no retry.
- **Write outside a transaction** `counter = expr`: writes directly, taking effect immediately, bypassing the transaction protocol.

```1y
let counter = shared 0;
counter = counter + 1;   // outside a transaction: a direct, non-atomic read-modify-write
```

Access outside a transaction is an "escape hatch," suited to single-threaded initialization, one-time assignment, and other scenarios known to be free of contention. **Whenever there is concurrent access, put it inside `transact`**, otherwise you bypass STM's isolation guarantees and reintroduce data races.

## A Complete Example: A Concurrency-Safe Counter

```1y
let counter = shared 0;

// Multiple transactions increment concurrently; STM guarantees the final count is correct
fn bump() {
    transact {
        let v = counter + 1;
        counter = v;
        v
    }
};

println("after increment: " + str(bump()));
println("after increment: " + str(bump()));
```

Even if `bump` is called concurrently, each transaction's "read-modify-write" is atomic: conflicting transactions retry automatically, and the final value of `counter` is exactly the number of calls — no lost updates. Compared with a lock-based version — where you'd carefully pick lock granularity and worry about deadlock — the STM version has almost no concurrency details left for you to fret over.

## Summary

| Element | Syntax | Purpose |
|---------|--------|---------|
| Create cell | `shared expr` | Create a transactional cell |
| Read | `counter` | Read current value (snapshot inside a transaction) |
| Write | `counter = expr` | Write a value (enrolled in commit inside a transaction) |
| Transaction | `transact { ... }` | Snapshot isolation + atomic commit |
| Retry | `retry` | Proactively rerun the transaction (cap 64) |
| Nesting | `transact` inside `transact` | Inner commits to the outer |

The value of STM is that **atomicity composes**: you can combine two independent transactions into a larger one without worrying about lock ordering. Coupled with 1y's immutable data, transaction retries are idempotent — both correct and efficient. Use [Actors](./actors) when state fits encapsulation; use STM when you need to coordinate shared state across boundaries. Together they form 1y's concurrency model.
