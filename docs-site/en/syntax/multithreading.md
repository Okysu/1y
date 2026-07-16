---
title: Multi-threading
---

# Multi-threading

1y provides user-facing multi-threading via the built-in `parallel` module. Built on the `WorkerPool` (N worker threads, one per CPU core), each worker pre-loads the entry file's definitions and stays alive to accept function calls.

## API

| Function | Signature | Description |
|----------|-----------|-------------|
| `parallel.cores` | `() -> Int` | Number of CPU cores |
| `parallel.call` | `(Str, Vec) -> Value` | Synchronously call a named function |
| `parallel.spawn` | `(Str, Vec) -> Handle` | Asynchronously call, return a handle |
| `parallel.join` | `(Handle) -> Value` | Wait for a spawned task |
| `parallel.map` | `(Str, Vec<Vec>) -> Vec` | Parallel mapping |

## Synchronous Call

```1y
fn heavy_compute(n) {
    let s = 0;
    let i = 0;
    while i < n { s = s + i; i = i + 1 };
    s
}

// Blocks until the worker finishes, returns the result.
let r = parallel.call("heavy_compute", [1000000]);
```

## Asynchronous Call

```1y
// Returns immediately with a handle.
let h1 = parallel.spawn("heavy_compute", [1000000]);
let h2 = parallel.spawn("heavy_compute", [2000000]);

// Block until each result is ready.
let r1 = parallel.join(h1);
let r2 = parallel.join(h2);
```

## Parallel Map

```1y
// All four calls run concurrently on different workers.
let results = parallel.map("heavy_compute", [[1000], [2000], [3000], [4000]]);
```

## Constraints

- Functions are called **by name** (string), not by closure reference
- Arguments and return values must be `SendValue`-compatible: Int, Str, Bool, Nil, Vec, Map, Set, Variant, Struct
- Functions, shared cells, actors, tasks, and opaque resources cannot cross thread boundaries
- Worker threads load only definitions (FuncDef, ActorDef, TypeDef, EnumDef, Import); side-effect statements are not re-run

## How It Works

1. **WorkerPool**: N worker threads, each with its own `Interpreter` (Rc-based, !Send)
2. **Pre-loading**: workers load the entry file's definitions on startup
3. **Job dispatch**: via a shared `mpsc` channel; any worker can pick up the next job
4. **Cross-thread communication**: uses `SendValue` (a Send+Sync subset of Value)

## Relationship to Actors

The `parallel` module is for **CPU-bound parallelism** (multi-core computation), while Actors are for **concurrent I/O** (connection management, message passing). They complement each other:

- `parallel.map` is ideal for parallel computation (e.g. batch data processing)
- Actors are ideal for concurrent I/O (e.g. HTTP server with one actor per connection)
