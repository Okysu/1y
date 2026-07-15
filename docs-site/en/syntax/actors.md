---
title: Actor Model
---

# Actor Model

The Actor is the first-class citizen of 1y concurrency. An Actor is a lightweight unit of execution with **isolated state** that interacts with the outside world exclusively through **message passing**, processing one message at a time. This "isolation plus messages" model eliminates data races at the root: if state is never shared, it never needs synchronizing.

This page walks through the complete syntax of Actors in 1y: how to create one with `spawn`, how to send messages with `!` and `?`, how to receive with `receive` and reply with `reply`, how isolated state and the event loop work, and finally the declarative `on` handler form.

## Creating an Actor: spawn

`spawn(initial_state) { body }` creates and starts an Actor. `initial_state` is the Actor's initial state, accessible inside `body` through a variable named `state`. The `body` is typically a `loop` that continuously waits for and handles messages.

```1y
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => state = state + n,
            Get => reply(state)
        }
    }
};
```

A few key points:

- `spawn` returns an **Actor handle** immediately, which you later talk to via `!` / `?`.
- The initial state `0` is **moved** into the Actor; from then on only the Actor itself can read or write it — the outside can never touch it.
- `body` runs in the Actor's own execution context, fully isolated from the caller.

## Sending Messages: ! and ?

1y provides two message-sending operators, each corresponding to a distinct communication semantic.

### Fire-and-forget: !

`actor ! Message(args)` drops a message into the Actor's mailbox and returns immediately, **without waiting** for a result.

```1y
counter ! Inc(5);      # tell the counter to add 5; don't care when it's done
```

`!` is for **command-style** messages: you only care that the request is "in the system," not about getting a result back. It is non-blocking, so the sender can move on to other work. A typical use is driving a long-running service — send a command and trust it will eventually be handled.

### Request/reply: ?

`actor ? Message(args)` sends a message and **blocks waiting for a reply**. Under the hood it is a `!` plus an implicit reply channel; when the Actor handles this message it uses `reply` to send the result back, and that reply becomes the return value of `?`.

```1y
let current = counter ? Get;   # blocks until the Actor replies
print(current);
```

`?` is for **query-style** messages: you need the Actor's state or the outcome of a computation. Note that `?` blocks the calling thread until a reply arrives, so inside an event loop you should avoid `?`-ing a message that takes a long time to reply, lest you starve other tasks. A good rule of thumb: treat `?` like a synchronous function call, and only issue it when you are confident the Actor will `reply` promptly.

## Receiving Messages: receive

`receive { Pattern => handler, ... }` blocks the current Actor until a message arrives that matches one of the patterns, then runs the corresponding `handler`. Patterns are separated by commas.

```1y
receive {
    Inc(n) => state = state + n,
    Dec(n) => state = state - n,
    Get => reply(state),
    Reset => state = 0
}
```

`receive` semantics:

- **Blocking**: until a matching message arrives, the Actor is suspended and consumes no CPU.
- **Pattern matching**: the message's constructor name (such as `Inc`, `Get`) and arguments must match. 1y's full pattern-matching power (literals, destructuring, guards, etc.) applies inside `receive`.
- **One at a time**: each `receive` handles exactly one message. Place `receive` inside a `loop` to serve continuously.

`receive` is usually written inside a `loop`, forming a long-running service cycle:

```1y
loop {
    receive {
        Inc(n) => state = state + n,
        Get => reply(state)
    }
}
```

## Replying: reply

`reply expr` is used, while handling a message that came in via `?`, to send `expr` back to the caller as the reply.

```1y
Get => reply(state)
```

Key points:

- `reply` only makes sense for requests initiated with `?`. Calling `reply` for a message that arrived via `!` has no effect — no one is waiting for a reply.
- A handler should `reply` at most once. If a handler never calls `reply`, the caller that issued `?` will block forever.
- Code after `reply` still runs, but it is conventional to place `reply` at the end of a handler for clarity.

## Isolated State

Every Actor has fully isolated state, accessed through the built-in `state` variable. This isolation is the core guarantee of 1y's concurrency safety:

- **The outside cannot read or write** an Actor's state directly. The only way to influence state is to send a message.
- **An Actor processes one message at a time**, so reads and writes to `state` are never concurrent.

```1y
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => state = state + n,
            Get => reply(state)
        }
    }
};
# the outside cannot read counter's state directly; only counter ? Get
```

Because of this, **when you write code inside an Actor, it is as if you were writing single-threaded code** — no locks, no atomics, no memory ordering to worry about. This is the single biggest mental simplification of the Actor model. When state needs to be shared, you don't lock it; you just decide *who* holds it — put it inside an Actor and let everyone interact with it through messages.

## The Event Loop

Actors in 1y **run single-threaded, multiplexed by an event loop**. Concretely:

- An Actor is not an OS thread; it is a lightweight, suspendable/resumable unit of execution.
- The event loop schedules them: when an Actor's mailbox has a message and the Actor is runnable, the loop resumes it, lets it handle one message, suspends it again, and moves on to the next Actor.
- Because it is single-threaded, there is no preemptive concurrency between Actors; accesses to `state` are inherently serial.

This model means a great many Actors can coexist without dedicating an OS thread to each. A `?` blocks that Actor's own execution flow, not the whole event loop — as long as other Actors have messages to process, the loop keeps making progress. It also means Actors do **not truly run in parallel**: if you need CPU-intensive parallel computation, slice the work into messages dispatched to several Actors and let the event loop interleave them, rather than expecting them to run simultaneously on multiple cores.

## on Handlers: An Alternative to receive

For service-style Actors with a fixed set of message types, spelling out each `receive` can feel verbose. 1y offers `on` handlers as an alternative: `on Name(params) { body }` declares a handler function for each kind of message directly.

```1y
let counter = spawn(0) {
    on Inc(n) {
        state = state + n
    }
    on Get {
        reply(state)
    }
};
```

The relationship between `on` and `receive`:

- `on` is equivalent to an automatically expanded `loop { receive { ... } }`; 1y generates a matching `receive` branch for each `on` and loops automatically.
- Inside an `on` handler you use `reply` to answer `?` requests and `state` to read and write isolated state, exactly as in `receive`.
- Choosing `receive` versus `on` is a matter of style: `receive` is more explicit and suits cases needing complex pattern matching or one-off receives; `on` is more declarative and suits services with a fixed message set.

## A Complete Example: A Capped Counter

The following slightly fuller example shows `spawn`, `!`, `?`, `receive`, and `reply` working together:

```1y
import io;

# A counter with an upper bound: refuses to exceed the cap
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => {
                if state + n <= 100 {
                    state = state + n;
                    reply(true)
                } else {
                    reply(false)
                }
            },
            Get => reply(state)
        }
    }
};

counter ? Inc(30);       # returns true; state is now 30
counter ? Inc(80);       # returns false (30+80 exceeds 100); state unchanged
let now = counter ? Get; # returns 30
println("current count: " + str(now));
```

Note that `Inc` here uses `?` rather than `!`: we want to know whether the increment succeeded, so we need `reply(true/false)`. If it were simply "just add it," `counter ! Inc(30)` would do, and the Actor would not need to `reply` at all.

## Summary

| Element | Syntax | Purpose |
|---------|--------|---------|
| Create | `spawn(state) { body }` | Create an Actor, return a handle |
| Fire-and-forget | `actor ! Msg(args)` | Deliver a message, don't wait |
| Request/reply | `actor ? Msg(args)` | Deliver a message and block for a reply |
| Receive | `receive { P => h, ... }` | Block for a matching message |
| Reply | `reply expr` | Send a result back to a `?` caller |
| Handler | `on Name(p) { body }` | Declarative message handling |
| State | `state` | The Actor's isolated state variable |

The Actor model replaces "shared plus locked" with "isolated plus messaged." Encapsulate mutable state inside an Actor, substitute messages for direct access, and concurrency shifts from "where do I put the lock" to "who messages whom" — inherently clearer and more composable. When you genuinely need to coordinate multiple pieces of state atomically, reach for the next chapter on [Software Transactional Memory](./stm).
