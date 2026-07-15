---
title: Actor KV Store
---

# Actor KV Store

A key-value store is one of the classic concurrency exercises: it has state, read/write contention, and needs a clear API boundary. This example builds an in-memory key-value store from scratch with 1y's Actor model, and you will see how "isolation plus message passing" makes a program that would otherwise need locks read like sequential code. We start with a single Actor, grow it into a full `Get`/`Set`/`Delete` interface, discuss the trade-off between "fire-and-forget" and "request/reply," and finally use multiple Actors for namespace isolation.

## A Quick Recap of the Actor Model

In 1y, an Actor is a lightweight process with private state. It receives messages from a mailbox one at a time and processes them sequentially, so **its own state is never accessed concurrently**. You define an Actor with the `actor` keyword and create an instance with `spawn`:

```1y
actor Counter {
    state count = 0
    on inc() { count = count + 1; reply(count) }
}

let c = spawn Counter();
let n = c ? inc();      // request/reply, returns 1
```

- `state count = 0` declares the Actor's private state, initialized to `0`. This state is completely invisible from the outside; it can only be touched indirectly through messages.
- `on inc() { ... }` defines a message handler: when an `inc` message arrives, the code in the braces runs.
- `reply(count)` returns a value to the sender. Only messages sent with `?` (request/reply) wait for this reply.
- `spawn Counter()` creates an instance of `Counter` and returns a handle to it.
- `c ? inc()` sends the `inc` message and **blocks until the reply** arrives; the return value is whatever `reply` produced.

## A First KV Actor

Let us put the core storage logic inside a single Actor. The state is a `Map`, and the messages are `put` (write), `get` (read), `has` (check whether a key exists), `delete` (remove), and `size` (return the entry count):

```1y
actor KVStore {
    // Note: `{}` parses as an empty block, not an empty Map.
    // Use this trick to get a genuinely empty Map as the initial state.
    state data = dissoc({ __init: 0 }, "__init")

    on put(key, val) { data = assoc(data, key, val); reply(nil) }
    on get(key) { reply(get(data, key)) }
    on has(key) { reply(get(data, key) != nil) }
    on delete(key) { data = dissoc(data, key); reply(nil) }
    on size() { reply(count(data)) }
}

let store = spawn KVStore();
store ? put("name", "alice");
store ? put("city", "paris");
println(store ? get("name"));          // alice
println(str(store ? has("city")));     // true
store ? delete("city");
println(str(store ? has("city")));     // false
println(str(store ? size()));          // 1
```

A few crucial details deserve a step-by-step explanation.

**The empty Map for initial state.** 1y parses `{}` as an empty code block (whose value is `Nil`), not as an empty Map. This is a historical syntactic trade-off. To make `data` a real Map, we use an idiom: construct `{"__init": 0}` first, then `dissoc` the placeholder key away, leaving a bona fide empty Map. In your own programs, as long as the initial state is non-empty, you can simply write `{"k": v}`.

**Updating state with `assoc`.** 1y's `Map` is persistent: `assoc(data, key, val)` does not mutate `data` in place — it returns a new Map that **shares most of its structure**. We write `data = assoc(...)` to "point" the Actor's state at that new Map. Because only the current Actor is processing a message, this assignment is safe — no other execution flow ever sees an intermediate state.

**The dual meaning of `get`.** Notice that `get` is both a message name and a built-in function name. In `on get(key) { reply(get(data, key)) }`, the first is the message label and the second is the lookup function; they never collide. `get(data, key)` returns `nil` when the key is absent, which is exactly what `has` relies on with its `!= nil` check.

## Fire-and-Forget vs Request/Reply

There are two ways to send to an Actor, and the choice depends on whether you care about the result:

```1y
// Fire-and-forget: do not wait for a reply
store ! put("name", "bob");

// Request/reply: block until reply's value comes back
let v = store ? get("name");
```

- **`!` (fire-and-forget)** drops the message into the mailbox and returns immediately, leaving the sender free to continue. It suits "command"-style messages — telling an Actor to do something without needing immediate confirmation. In the example above, `put` could well be sent with `!`, since we only care that the write happens, not about its (`nil`) reply.
- **`?` (request/reply)** sends the message and blocks until the Actor calls `reply` inside its handler. It suits "query"-style messages — when you need the Actor's state or a computed result.

A practical guideline: **use `?` for queries, and choose per-case for writes.** If you must read back a confirmation right after writing, use `?`; if you are bulk-loading data, use `!` so the sender does not block. One thing to be aware of: messages sent with `!` are processed during an end-of-program "drain" phase, so a `?` immediately following a `!` may not yet see the `!`'s effect — this is a timing property of the Actor mailbox, not a bug.

## Namespaces: Isolating State with Multiple Actors

A natural advantage of the Actor model is that a single Actor definition can be `spawn`ed into many mutually isolated instances. We use this for namespace isolation — each namespace is its own `KVStore` instance:

```1y
let users = spawn KVStore();
let sessions = spawn KVStore();

// The two stores never interfere
users ? put("alice", "admin");
sessions ? put("token-42", "alice");

println(users ? get("alice"));          // admin
println(sessions ? get("token-42"));    // alice
println(str(users ? size()));           // 1
println(str(sessions ? size()));        // 1
```

`users` and `sessions` each own an independent `data` Map. Even if both Actors receive messages at the same time, their states cannot pollute one another, because Actor state is isolated by definition — that is "isolation is simpler than synchronization" made concrete. To extend this into arbitrary namespaces, add a "manager Actor" that keeps a `Map<namespace, KVStore handle>` and forwards each request to the appropriate child Actor.

## When to Build Storage with Actors

The Actor pattern is well suited to **long-lived, stateful services with clear boundaries.** Its costs: cross-Actor atomic operations are not intuitive (an "atomically move key from A to B" needs an extra protocol), and queries block on response. When what you need is a one-off atomic update spanning several data structures, the STM introduced in [Transactional Counter](./transactional-counter) is usually smoother. The two are not either/or but complementary — the rule of thumb is the same: **reach for Actors by default, and turn to STM only when several shared cells must change atomically together.**
