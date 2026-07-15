---
title: Why No async/await
---

# Why No async/await

`async/await` has become a standard feature of modern languages: JavaScript, Python, Rust, C#, and Kotlin have all adopted it. Against this tide, 1y chooses **not** to implement async/await. This is not an oversight, nor a lack of technical capability — it is a deliberate design decision. This chapter explains the reasoning.

## What Problem async/await Solves

To understand 1y's choice, first acknowledge the real problem async/await solves: **when handling a lot of I/O in a single thread, blocking I/O wastes thread resources**. A thread blocked on a socket read occupies an OS thread while doing nothing. To handle ten thousand connections simultaneously, you either spawn ten thousand threads (memory blowup) or use non-blocking I/O with an event loop.

`async/await` is syntactic sugar for "writing non-blocking code in a synchronous style": the function is compiled into a state machine that yields control at `await` points, letting the event loop service other connections. This genuinely makes high-concurrency server code readable.

## What Complexity async/await Introduces

However, the price of this sugar is **function coloring**. Once async is introduced, functions split into two worlds:

- **async functions** can only be called by other async functions, or scheduled inside an async runtime;
- **plain functions** cannot call async functions (or can only block on them).

This split is contagious: once a low-level function becomes async, every function on the call chain must become async too. The community calls it "async contagion." Its practical consequences include:

- **API fragmentation**: the same feature often needs both sync and async interfaces (e.g. Rust's `Read` vs `AsyncRead`).
- **Type complexity**: Rust's `Future` pulls in `Pin`, `Poll`, `Waker`, and `Send` bounds — enough to deter newcomers.
- **Executor fragmentation**: tokio, async-std, smol, and embassy are mutually incompatible; choosing a runtime means choosing an ecosystem.
- **Debugging difficulty**: async stack traces are broken; they often point into the runtime internals rather than business code.
- **Vague cancellation semantics**: when a future is dropped, the timing of resource cleanup is unintuitive and prone to leaks.

These complexities are not "you get used to it" — they are **structural**, stemming from the basic fact that functions come in two colors.

## 1y's Alternative: Blocking I/O + Actors

1y solves the high-concurrency I/O problem a different way: **keep blocking I/O, and use Actors for concurrency**.

The key insight is that an Actor is itself a lightweight unit of execution. 1y's runtime multiplexes a large number of Actors onto a pool of OS threads; when one Actor blocks on I/O, the runtime schedules other Actors onto other threads. From the programmer's view, the code is a plain blocking call; from the runtime's view, concurrency is determined by the number of Actors, not the number of threads.

```1y
actor Fetcher {
  on Fetch(url, reply) {
    # This is a blocking call, but the Actor model means it won't
    # bring down the whole system.
    let body = http_get(url)
    reply ! body
  }
}

# Spin up ten thousand Fetchers to fetch concurrently — effortless.
let fetchers = List.map(urls, url => {
  let f = spawn Fetcher
  f ! Fetch(url, self)
})
```

Note the simplicity: **no `async`, no `await`, no `Pin`, no runtime choice**. The code inside each Actor is ordinary sequential code that reads like a synchronous program. Concurrency shows up as "spawned ten thousand Actors," not as "added an `async` modifier to each function."

## A Unified Mental Model

The biggest hidden cost of async/await is not writing the code — it is the **fragmentation of the mental model**. The programmer must constantly ask:

- Is this function async?
- Should I `.await` it or `spawn` it?
- Is this future `Send`? Can it cross threads?
- Is this lock an async lock or a sync lock?
- Will this code block the runtime?

In 1y, these questions **do not exist**. All functions are one color — plain functions. All I/O is blocking, but what it blocks is an Actor, not the whole system. All "concurrency" is expressed by spawning Actors. There is a single mental model: **"spawn an Actor to handle this."**

## Trade-offs: What We Give Up

To be honest, this decision has a cost.

- **Weaker fine-grained concurrency control.** In some scenarios (such as finely interleaving multiple I/O operations inside one event loop), async/await is genuinely more expressive. 1y programmers must recast these scenarios as multiple cooperating Actors, which is sometimes more verbose.
- **A lower ceiling in extreme-performance scenarios.** Zero-copy, zero-allocation async frameworks (such as certain Rust libraries) have an edge under extreme performance. 1y's Actor scheduling overhead is small but not zero.
- **Interop with the async ecosystem needs an FFI bridge.** When reusing Rust's async libraries, you must wrap async calls into blocking calls at the FFI boundary.

## Why This Trade-off Is Worth It

1y's target users are **programmers writing business logic**, not engineers building high-performance networking frameworks. For the former, mental simplicity matters more than peak throughput; correctness matters more than speed; being able to trust concurrency primitives matters more than fine-tuning scheduling.

`async/await` exposes the engineering problem of "how to use threads efficiently" to every application programmer; 1y leaves that problem to the runtime and lets the programmer focus only on "what do I want to do concurrently." This is a value ordering — **simplicity first, performance second** — with performance backstopped by a runtime implemented in Rust.

When the complexity of the async ecosystem has turned "learning Rust's async" into a discipline of its own, 1y wants to show that for the overwhelming majority of application scenarios, **blocking I/O + Actors is good enough — and far more pleasant**. This is not a denial of async/await; it is a question of whether it should be the default for every language. 1y offers a different answer.
