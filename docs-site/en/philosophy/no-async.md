---
title: Colorless Async — Why No `async` Keyword
---

# Colorless Async — Why No `async` Keyword

`async/await` has become a standard feature of modern languages: JavaScript, Python, Rust, C#, and Kotlin have all adopted it. 1y takes a different path: **there is no `async` keyword, but there is `await`**. This is not a contradiction — it is *colorless async*, the same approach Zig takes. This chapter explains why.

## The Real Problem: Function Coloring

The deepest cost of `async/await` is not syntax — it is **function coloring**. Once `async` exists, functions split into two worlds:

- **async functions** can only be called by other async functions, or scheduled inside an async runtime;
- **plain functions** cannot call async functions (or can only block on them).

This split is contagious: once a low-level function becomes `async`, every function on the call chain must become `async` too. The community calls it "async contagion." Its practical consequences include:

- **API fragmentation**: the same feature often needs both sync and async interfaces (e.g. Rust's `Read` vs `AsyncRead`).
- **Type complexity**: Rust's `Future` pulls in `Pin`, `Poll`, `Waker`, and `Send` bounds — enough to deter newcomers.
- **Executor fragmentation**: tokio, async-std, smol, and embassy are mutually incompatible; choosing a runtime means choosing an ecosystem.
- **Debugging difficulty**: async stack traces are broken; they often point into the runtime internals rather than business code.
- **Vague cancellation semantics**: when a future is dropped, the timing of resource cleanup is unintuitive and prone to leaks.

These complexities are not "you get used to it" — they are **structural**, stemming from the basic fact that functions come in two colors.

## 1y's Answer: Colorless Async (Zig-style)

1y solves the concurrency problem without coloring functions. The rule is simple:

> **There is no `async` keyword. Any function can use `await`. A function is just a function.**

This is the same insight behind Zig's async model. The compiler/runtime decides whether a call suspends or not based on *what is being awaited*, not based on a marker on the function definition. Concretely:

- A function that calls `await socket.read_async(stream, n)` **may** suspend at that point — but it is still a plain `fn`, callable from anywhere.
- A function that calls only synchronous code never suspends — no marker needed.
- The same function can do both: synchronous work, then `await` a Task, then more synchronous work.

```1y
// A plain fn. No `async` marker. Yet it can `await`.
fn handler(req) {
    let stream = get(req, "stream");
    // Suspends the coroutine if no data is available yet.
    // Other connections keep being served while this one waits.
    let raw = await socket.read_async(stream, 65536);
    // Plain synchronous call — no marker needed.
    let parsed = parse_request(raw);
    // Another await — e.g. pacing an SSE stream.
    await process.sleep_async(500);
    build_response(parsed)
}
```

Note what is **not** here: no `async fn`, no `.await` suffix, no `Pin<Box<dyn Future>>`, no runtime choice. The function reads exactly like synchronous code. The only new keyword is `await`, and it works in *any* function.

## How It Works: Tasks + Coroutines

Under the hood, 1y uses stackful coroutines (via the `corosensei` library) with a cooperative scheduler:

1. **`Task`** — a value representing an async operation. `socket.read_async(stream, n)` returns `Task<Str|Nil>`. `process.sleep_async(ms)` returns `Task<Nil>`. A Task is in one of three states: `Pending`, `Ready(value)`, or `Consumed`.

2. **`await task`** — suspends the current coroutine and registers the Task with the scheduler. When the Task becomes `Ready`, the scheduler resumes the coroutine with the Task's value. If called outside a coroutine (top-level), `await` falls back to a synchronous busy-poll.

3. **`yield`** — the concurrency heartbeat. Inside an Actor-based event loop (e.g. `http.serve`), `yield` drains all pending `!` messages by spawning a coroutine per message and running the scheduler. Coroutines that `await` a not-yet-ready Task stay parked; the scheduler polls their Tasks each tick and resumes them when ready.

4. **Scheduler** — single-threaded, cooperative. `run_until_complete` runs all ready coroutines, then polls parked Tasks, repeating until everything is done or everything is parked (waiting on I/O). No OS threads are spawned for concurrency.

The key property: **when a coroutine awaits, other coroutines run**. A slow handler does not block other in-flight connections.

## Why This Beats Colored async/await

| | Colored `async/await` | 1y's colorless `await` |
|---|---|---|
| Function marker | `async fn` required | plain `fn` — no marker |
| Calling an async fn | must be in an async context | any function can `await` |
| Contagion | yes — async spreads up the call chain | none — functions are one color |
| Runtime | tokio/async-std/smol (incompatible) | built-in scheduler, no choice needed |
| Type complexity | `Future` + `Pin` + `Poll` + `Waker` | `Task` — a plain value |
| Mental model | "is this async? should I `.await`?" | "if it returns a Task, `await` it" |

## Relationship to Actors

Colorless async does not replace Actors — it complements them. Actors provide *structured concurrency* (isolation, message passing, supervision). `await` provides *fine-grained suspension* inside a single Actor's handler.

Typical pattern: spawn one Actor per connection, let the handler `await` async I/O. The Actor model gives you isolation and message-based APIs; `await` gives you non-blocking I/O inside a handler without splitting your functions into sync/async worlds.

```1y
actor Connection {
    on Handle(stream, handler) {
        // await inside an Actor handler — colorless async.
        let raw = await socket.read_async(stream, 65536);
        let resp = handler(parse_request(raw));
        socket.write(stream, build_response(resp));
        socket.close(stream)
    }
}

// The accept loop: spawn Connection actors, yield to advance them.
fn serve(addr, handler) {
    let listener = socket.listen(addr);
    socket.set_nonblocking(listener, true);
    loop {
        let stream = socket.accept(listener);
        if is_nil(stream) {
            try { yield } rescue { nil };
            process.sleep_ms(1)
        } else {
            let conn = spawn Connection();
            conn ! Handle(stream, handler)
        };
        try { yield } rescue { nil }
    }
}
```

## Trade-offs: What We Give Up

To be honest, this design has costs.

- **Cooperative, not preemptive.** A coroutine that never `await`s will run to completion. There is no preemption. Long CPU-bound loops should yield periodically or be offloaded to `process.exec`.
- **Single-threaded (for now).** True multi-core parallelism requires multiple processes (via `process`) or future work on a multi-threaded scheduler. Colorless async gives concurrency, not parallelism.
- **Event-driven I/O.** The scheduler uses `mio` (`epoll`/`kqueue`/IOCP) to wait only on readiness of registered streams, so parked Tasks are polled only when the OS reports them ready rather than every `yield`.
- **Task combinators are minimal.** `await` is the core primitive; `task_all([t1, ...])`, `task_any([t1, ...])`, and `task_ready(value)` compose Tasks. For long-lived concurrent state, prefer Actors. This is deliberate minimalism — matching Zig's philosophy of one primitive well.

## Why This Trade-off Is Worth It

1y's target users are **programmers writing business logic**, not engineers building high-performance networking frameworks. For the former, mental simplicity matters more than peak throughput; correctness matters more than speed; being able to trust concurrency primitives matters more than fine-tuning scheduling.

Colored `async/await` exposes the engineering problem of "how to use threads efficiently" to every application programmer, and then forces them to maintain two parallel function universes. 1y leaves the threading problem to the runtime and lets the programmer write **one kind of function** that can do both synchronous work and `await` async I/O — with zero ceremony.

This is not a denial of async/await's power; it is a rejection of **function coloring**. Zig proved that you can have async suspension without the color split. 1y follows that path: **`await` without `async`, concurrency without colors.**
