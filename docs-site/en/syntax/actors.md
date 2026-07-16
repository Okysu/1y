---
title: Actor Model
---

# Actor Model

The Actor is the first-class citizen of 1y concurrency. An Actor is a lightweight unit of execution with **isolated state** that interacts with the outside world exclusively through **message passing**, processing one message at a time. This "isolation plus messages" model eliminates data races at the root: if state is never shared, it never needs synchronizing.

This page walks through the complete syntax of Actors in 1y: how to define one with `actor`, how to start it with `spawn`, how to send messages with `!` and `?`, how to reply with `reply`, how isolated state and the event loop work, and the declarative `on` handler form.

## Defining an Actor: actor + spawn

An Actor is defined with `actor Name { body }`, where the body contains `state` declarations, `on` handlers, and `fn` definitions. An instance is created and started with `spawn Name(args)`, which returns an **Actor handle** you later talk to via `!` / `?`.

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on get() { reply(count) }
}

let counter = spawn Counter();
```

A few key points:

- `spawn Name(args)` returns an **Actor handle** immediately, which you later talk to via `!` / `?`.
- The Actor's state is initialized by the `state` declarations inside the body. Each `state name = value` creates an isolated, mutable binding that only the Actor itself can read or write — the outside can never touch it.
- The body runs in the Actor's own execution context, fully isolated from the caller.

## Sending Messages: ! and ?

1y provides two message-sending operators, each corresponding to a distinct communication semantic.

### Fire-and-forget: !

`actor ! Msg(args)` drops a message into the Actor's mailbox and returns immediately, **without waiting** for a result.

```1y
counter ! inc(5);      // tell the counter to add 5; don't care when it's done
```

`!` is for **command-style** messages: you only care that the request is "in the system," not about getting a result back. It is non-blocking, so the sender can move on to other work. A typical use is driving a long-running service — send a command and trust it will eventually be handled.

### Request/reply: ?

`actor ? Msg(args)` sends a message and **blocks waiting for a reply**. Under the hood it is a `!` plus an implicit reply channel; when the Actor handles this message it uses `reply` to send the result back, and that reply becomes the return value of `?`.

```1y
let current = counter ? get();   // blocks until the Actor replies
print(current);
```

`?` is for **query-style** messages: you need the Actor's state or the outcome of a computation. Note that `?` blocks the calling thread until a reply arrives, so inside an event loop you should avoid `?`-ing a message that takes a long time to reply, lest you starve other tasks. A good rule of thumb: treat `?` like a synchronous function call, and only issue it when you are confident the Actor will `reply` promptly.

## Handling Messages: on

Each kind of message an Actor accepts is declared with an `on` handler: `on Name(params) { body }`. When a message is sent to the Actor, 1y dispatches it to the `on` handler whose name matches the message's constructor name. Handlers always require parentheses (use `on Get() { ... }` for a message with no arguments).

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on dec(n) { count = count - n }
    on get() { reply(count) }
    on reset() { count = 0 }
}
```

`on` semantics:

- **Dispatch by name**: the message's constructor name (such as `inc`, `get`) selects the handler; the message's arguments are bound to the handler's parameters.
- **One at a time**: the Actor processes one message at a time, so reads and writes to its `state` bindings are never concurrent.
- **Pattern matching in the body**: although dispatch is by name, the handler body can use `match` and any of 1y's pattern-matching power (literals, destructuring, guards, etc.) on the parameters or other values.

## Replying: reply

`reply expr` is used, while handling a message that came in via `?`, to send `expr` back to the caller as the reply.

```1y
on get() { reply(count) }
```

Key points:

- `reply` only makes sense for requests initiated with `?`. Calling `reply` for a message that arrived via `!` has no effect — no one is waiting for a reply.
- A handler should `reply` at most once. If a handler never calls `reply`, the caller that issued `?` will block forever.
- Code after `reply` still runs, but it is conventional to place `reply` at the end of a handler for clarity.

## Isolated State

Every Actor has fully isolated state, declared with `state` bindings inside the actor body. This isolation is the core guarantee of 1y's concurrency safety:

- **The outside cannot read or write** an Actor's state directly. The only way to influence state is to send a message.
- **An Actor processes one message at a time**, so reads and writes to its `state` bindings are never concurrent.

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on get() { reply(count) }
}

let counter = spawn Counter();
// the outside cannot read counter's state directly; only counter ? get()
```

Because of this, **when you write code inside an Actor, it is as if you were writing single-threaded code** — no locks, no atomics, no memory ordering to worry about. This is the single biggest mental simplification of the Actor model. When state needs to be shared, you don't lock it; you just decide *who* holds it — put it inside an Actor and let everyone interact with it through messages.

## The Event Loop

Actors in 1y **run single-threaded, multiplexed by an event loop**. Concretely:

- An Actor is not an OS thread; it is a lightweight, suspendable/resumable unit of execution.
- The event loop schedules them: when an Actor's mailbox has a message and the Actor is runnable, the loop resumes it, lets it handle one message, suspends it again, and moves on to the next Actor.
- Because it is single-threaded, there is no preemptive concurrency between Actors; accesses to `state` are inherently serial.

This model means a great many Actors can coexist without dedicating an OS thread to each. A `?` blocks that Actor's own execution flow, not the whole event loop — as long as other Actors have messages to process, the loop keeps making progress. It also means Actors do **not truly run in parallel**: if you need CPU-intensive parallel computation, slice the work into messages dispatched to several Actors and let the event loop interleave them, rather than expecting them to run simultaneously on multiple cores.

To let a `!` (fire-and-forget) message be processed before the program exits, use `yield;` — it drains pending mailboxes inline. Otherwise `!` messages are drained at the end of the program.

## A Complete Example: A Capped Counter

The following slightly fuller example shows `actor`, `spawn`, `!`, `?`, and `reply` working together:

```1y
// A counter with an upper bound: refuses to exceed the cap
actor CappedCounter {
    state count = 0
    on inc(n) {
        if count + n <= 100 {
            count = count + n;
            reply(true)
        } else {
            reply(false)
        }
    }
    on get() { reply(count) }
}

let counter = spawn CappedCounter();
counter ? inc(30);       // returns true; count is now 30
counter ? inc(80);       // returns false (30+80 exceeds 100); count unchanged
let now = counter ? get(); // returns 30
println("current count: " + str(now));
```

Note that `inc` here uses `?` rather than `!`: we want to know whether the increment succeeded, so we need `reply(true/false)`. If it were simply "just add it," `counter ! inc(30)` would do, and the Actor would not need to `reply` at all.

## Summary

| Element | Syntax | Purpose |
|---------|--------|---------|
| Define | `actor Name { ... }` | Declare an Actor with state and handlers |
| Create | `spawn Name(args)` | Start an Actor instance, return a handle |
| Fire-and-forget | `actor ! Msg(args)` | Deliver a message, don't wait |
| Request/reply | `actor ? Msg(args)` | Deliver a message and block for a reply |
| Reply | `reply expr` | Send a result back to a `?` caller |
| Handler | `on Name(p) { body }` | Declarative message handling |
| State | `state name = value` | The Actor's isolated state binding |

The Actor model replaces "shared plus locked" with "isolated plus messaged." Encapsulate mutable state inside an Actor, substitute messages for direct access, and concurrency shifts from "where do I put the lock" to "who messages whom" — inherently clearer and more composable. When you genuinely need to coordinate multiple pieces of state atomically, reach for the next chapter on [Software Transactional Memory](./stm).
