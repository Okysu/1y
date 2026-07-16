---
title: Colorless Async
---

# Colorless Async

1y implements **Zig-style colorless async**: any `fn` can use `await` without an `async` keyword. There is no function coloring — synchronous and asynchronous calls use the same calling convention.

## Creating Tasks

A `Task` is produced by I/O operations that may block:

```1y
import socket;
import process;

// socket.read_async — suspends on WouldBlock, resumes on data
let data = await socket.read_async(stream, 65536);

// process.sleep_async — suspends for a duration
await process.sleep_async(500);
```

## Task Combinators

```1y
// task_ready(value) — immediately-ready Task
let t1 = task_ready(42);

// task_all([t1, t2, ...]) — resolves when all inputs resolve
let results = await task_all([t1, t2]);

// task_any([t1, t2, ...]) — resolves when any input resolves
let first = await task_any([t1, t2]);
```

## Why No `async` Keyword?

In languages with function coloring (Python `async def`, Rust `async fn`, JS `async`), you must annotate functions that may await, and callers must handle `Future`/`Promise` differently. This creates two worlds: sync and async, which don't compose freely.

1y uses **stackful coroutines** (`corosensei`): `await` suspends the entire call stack, so any function — even one written without knowing about async — can be called from within an `await`ing context, and any function can start `await`ing without changing its signature.

## HTTP Handler Example

```1y
import lib.http as http;

// This handler is just a regular fn — no `async` marker.
// It can `await` inside, and slow handlers do NOT block other connections.
fn handler(req) {
    await process.sleep_async(100);  // simulate slow work
    { "status": 200, "body": "done", "headers": [] }
}

http.serve("127.0.0.1:8080", handler)
```

## Event-Driven I/O

1y's scheduler uses `mio` (Linux: `epoll`, macOS: `kqueue`, Windows: IOCP) for event-driven I/O:

- `socket.accept_async(listener)` — suspends until a connection is pending
- `socket.read_async(stream, n)` — suspends until data is available

When a coroutine awaits, the scheduler runs other ready coroutines. A slow handler does not block other connections.

## How It Works

1. **Stackful coroutines** (`corosensei`): `await` suspends the entire call stack
2. **Cooperative scheduler** (`src/runtime/scheduler.rs`): maintains a list of parked coroutines
3. **Task sources**: `socket.read_async`, `process.sleep_async`, `task_ready`, `task_all`, `task_any`
4. **No markers**: a handler defined as `fn(req) { ... }` can `await` inside its body
