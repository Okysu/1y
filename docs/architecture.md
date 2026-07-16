# `1y` Architecture

This document describes the internal architecture of the `1y` interpreter
implementation as of Phase C (BEAM-style actor model + colorless async + yin).

## Overview

`1y` is a tree-walking interpreter. Source text flows through:

```
source string
    │
    ▼
┌─────────┐     ┌──────────┐     ┌──────────────┐
│  Lexer  │ ──▶ │  Parser  │ ──▶ │ Interpreter  │ ──▶ Value
└─────────┘     └──────────┘     └──────────────┘
   token.rs       mod.rs            mod.rs
```

There is no bytecode or JIT: the AST is walked directly. Persistent data
structures (from the `im` crate) make functional updates cheap.

## Crate Layout

```
src/
├── lib.rs              # Public API re-exports
├── main.rs             # CLI entry point (`1y run`, `1y parse`, etc.)
├── error.rs            # SourceError, ErrorReport
├── printer.rs          # AST pretty-printer
├── ast/
│   ├── mod.rs          # AST node definitions
│   └── span.rs         # Source positions
├── lexer/
│   ├── mod.rs          # Tokenizer
│   └── token.rs        # Token, TokenKind, Keyword
├── parser/
│   └── mod.rs          # Recursive-descent parser
├── runtime/            # Phase C: BEAM-style actor runtime
│   ├── mod.rs          # Module declarations
│   ├── scheduler.rs    # Stackful-coroutine scheduler (colorless async)
│   ├── registry.rs     # Global ActorPid → channel routing table
│   └── worker.rs       # WorkerPool: N-thread shared job queue
└── interpreter/
    ├── mod.rs          # Evaluator + module system + actor runtime + STM
    ├── env.rs          # Environment (scope chain)
    ├── error.rs        # InterpreterError
    ├── ops.rs          # Value operations (arithmetic, collection ops)
    ├── builtins.rs     # Global built-in functions
    └── stdlib/         # Standard library modules
        ├── mod.rs      # Module registry
        ├── env.rs      # Environment variables
        ├── io.rs       # File I/O
        ├── json.rs     # JSON parser/serializer (no serde)
        ├── process.rs  # Process control (sleep, sleep_async)
        ├── random.rs   # xorshift64 PRNG
        ├── serial.rs   # Serial port I/O
        ├── socket.rs   # TCP networking (read_async, non-blocking I/O)
        ├── crypto.rs   # Hashing, HMAC, CSPRNG
        ├── tls.rs      # TLS client (rustls)
        └── ffi.rs      # Dynamic library loading
```

User-facing libraries live in `lib/`:

```
lib/
├── http.1y             # Self-hosted HTTP/1.1 server (Actor + colorless async)
└── yin.1y              # Gin-inspired web framework built on lib.http
```

## Key Types

### `Value` (`src/value.rs`)

The tagged union for all `1y` values:

```rust
pub enum Value {
    Int(BigInt),           // arbitrary-precision integer
    Decimal(BigDecimal),   // arbitrary-precision decimal
    Str(Rc<str>),          // reference-counted UTF-8 string
    Bool(bool),
    Nil,
    Vec(Rc<Vec<Value>>),
    Map(Rc<HashMap<Value, Value>>),
    Set(Rc<HashSet<Value>>),
    Func(Rc<Closure>),     // user-defined function
    Native(Rc<NativeFn>),  // built-in function (fn pointer)
    Variant { name, args },// enum variant
    Struct { name, fields },
    Actor(Rc<Actor>),
    Shared(Rc<RefCell<SharedCell>>),  // STM cell
    Module(ModuleRef),
    Opaque(ResourceRef),   // native resource handle
    LazyImport { path },   // deferred module load
}
```

`Rc` is used throughout (not `Arc`) because the interpreter is single-threaded.
Actors run on separate OS threads but each has its own interpreter instance.

### `Interpreter` (`src/interpreter/mod.rs`)

Holds all interpreter state:

```rust
pub struct Interpreter {
    global: EnvRef,
    std_modules: HashMap<String, ModuleRef>,
    module_cache: HashMap<PathBuf, ModuleRef>,
    module_load_stack: Vec<PathBuf>,
    entry_dir: Option<PathBuf>,
    // Actor runtime (Phase C)
    live_actors: Vec<ActorRef>,
    pid_to_actor: HashMap<u64, ActorRef>,        // local Pid → ActorRef
    cross_inbox: Receiver<CrossEnvelope>,        // messages from other threads
    _cross_sender: Sender<CrossEnvelope>,        // registered in ActorRegistry
    // Colorless async (Phase 4.7)
    scheduler: Scheduler,                        // stackful coroutines
}
```

- `global`: the root environment containing builtins.
- `std_modules`: pre-built standard library modules, keyed by name.
- `module_cache`: file modules keyed by canonical path (prevents reload).
- `module_load_stack`: tracks the current import chain for cycle detection.
- `entry_dir`: the directory of the entry-point file, used for resolving
  relative module paths.
- `pid_to_actor`: maps Pids (from `ActorRegistry`) to local `ActorRef`s so
  cross-thread messages can be dispatched to the right actor.
- `cross_inbox`/`_cross_sender`: a channel pair bridging this interpreter
  thread with the global `ActorRegistry`, enabling cross-thread actor sends.
- `scheduler`: the stackful-coroutine scheduler (see Colorless Async below).

### Environment (`src/interpreter/env.rs`)

Environments form a scope chain via `Rc<RefCell<Env>>`:

```rust
pub struct Env {
    bindings: HashMap<String, Value>,
    parent: Option<EnvRef>,
}
```

Lookups walk the chain; `define` adds to the current scope; `assign` mutates an
existing binding anywhere in the chain.

## Module System

Module loading happens in `Interpreter::load_module`:

1. **Std module check**: if `path` matches a key in `std_modules`, return it.
2. **Path resolution**: `a.b.c` → `<entry_dir>/a/b/c.1y`.
3. **Cache check**: if the canonical path is in `module_cache`, return cached.
4. **Cycle detection**: push the path onto `module_load_stack`; if it's already
   there, raise a circular import error.
5. **Parse + eval**: read the file, parse it, evaluate top-level statements in
   a fresh child environment.
6. **Collect exports**: all top-level bindings become the module's exports.
7. **Cache**: store in `module_cache` and pop the load stack.

### Lazy Import

`lazy import foo;` binds `foo` to `Value::LazyImport { path }`. When `foo` is
first evaluated as an `Expr::Ident`, the interpreter loads the module and
replaces the binding with `Value::Module(...)`. Subsequent accesses skip
loading.

## Actor Runtime (Phase C: BEAM-style)

Actors are created with `spawn(initial_state) { body }`:

- Each actor runs on a `WorkerPool` thread (default: 1 worker) with a large
  stack (64 MB). The pool shares a single job queue.
- Messages are sent via `actor ! Message(args)` — they queue in the actor's
  mailbox.
- `receive { Pattern => handler }` blocks until a matching message arrives.
- `reply(value)` sends a reply to the caller (synchronous request/reply).
- `state` is bound in the actor's body and persists across `receive` calls.

Actors do NOT share memory; the only inter-actor communication is messages.
This avoids the need for locks in user code.

### Pid + Cross-thread Messaging (Phase C3)

Every spawned actor is assigned a unique `ActorPid` (an `Int`) via the global
`ActorRegistry` (`src/runtime/registry.rs`). The registry maps each Pid to an
`mpsc::Sender<CrossEnvelope>`, so a `pid ! msg` from any thread routes the
message to the owning interpreter's `cross_inbox`. The event loop drains
`cross_inbox` on each `yield` and dispatches envelopes to local actors.

- `ActorRegistry`: thread-safe global `HashMap<u64, Sender<CrossEnvelope>>`.
- `ActorPid`: a `Value` variant wrapping an `Int` id.
- `CrossEnvelope`: `Send + Sync` envelope carrying `(target_pid, message)`.
- `SendValue`: a `Send + Sync` subset of `Value` used as the message payload
  (Int, Bool, Str, Vec, Map, Nil, etc. — no `Rc`/`RefCell`).
- `pid_of(actor) -> Int`: builtin returning an actor's Pid.

### WorkerPool (Phase C2)

`src/runtime/worker.rs` provides a `WorkerPool` with N worker threads sharing
a single `mpsc` job queue. `main.rs` uses `WorkerPool::new(1)`. Each job runs
a source string in a fresh `Interpreter` on a worker thread, returning the
result via a channel. This abstracts thread spawning and can scale to N>1
workers for parallel actor execution in the future.

## Colorless Async (Zig-style)

`1y` implements **colorless async**: any `fn` can `await` a `Task` without an
`async` keyword. There is no function coloring — synchronous and asynchronous
calls use the same calling convention, so the mental burden of "is this
function async?" is eliminated.

### How it works

1. **Stackful coroutines** (`corosensei`): each `await` suspends the current
   call stack (not just the function) and yields control back to the
   scheduler. The full stack is preserved, so any deeply-nested call can
   suspend.
2. **Cooperative scheduler** (`src/runtime/scheduler.rs`): maintains a list
   of parked coroutines. On each `yield`, the scheduler polls their `Task`s
   and resumes any that are ready.
3. **Task sources**: `Task`s are produced by I/O operations that may block:
   - `socket.read_async(stream, n)` — suspends on `WouldBlock`, resumes on data
   - `process.sleep_async(ms)` — suspends for a duration
   - `task_ready(value)` — immediately ready
   - `task_all([t1, t2, ...])` / `task_any([t1, t2, ...])` — combinators
4. **No markers**: a handler defined as `fn(req) { ... }` can `await` inside
   its body just like a sync function calls a sub-function. The HTTP server
   passes such handlers to `http.serve` and they run inside a coroutine
   transparently.

### Why this matters for HTTP

In `lib/http.1y`, each connection runs in a spawned `Connection` actor. The
handler body may `await socket.read_async` (for slow clients) or
`await process.sleep_async` (for SSE pacing) **without blocking other
connections**. A slow handler suspends its coroutine; the scheduler runs
other ready coroutines in the meantime. This is verified by the benchmark
below (slow handler does not block fast handlers).

## The `yin` Web Framework

`lib/yin.1y` is a Gin-inspired web framework, self-hosted in pure `1y`. It
demonstrates that 1y's language features (shared cells, persistent
collections, colorless async, actors) are sufficient to build a real web
framework with no native extensions.

### Design

- **App** is a `shared` cell holding a `Map`. The `Map` contains three nested
  `shared` cells (`routes`, `param_routes`, `middlewares`) so that **route
  groups share the parent's route table** — registering on a group writes
  through to the same nested cells.
- **Context** is also a `shared` cell; `yin.json(ctx, ...)` mutates it in
  place, so handlers do not need to return the context.
- **Shared cell parameter passing**: when a bare identifier bound to a
  `SharedRef` is passed as a function argument, `1y` passes the `SharedRef`
  itself (not a dereferenced copy). This lets `register(app, ...)` write
  through to the nested shared cells. Native builtins auto-deref `SharedRef`
  args, so `push`/`count`/`assoc` still work transparently.

### Features

- Exact routes, param routes (`:id`), route groups with shared tables
- Middleware chain with `next()` continuation
- JSON / HTML / text response helpers
- `GET` / `POST` / `PUT` / `DELETE` method helpers

### Usage

```1y
import lib.yin as yin;

let app = yin.new();
yin.use(app, fn(ctx, next) { println(ctx.req.path); next() });
yin.get(app, "/ping", fn(ctx) { yin.json(ctx, 200, { "message": "pong" }) });
yin.get(app, "/users/:id", fn(ctx) {
    yin.json(ctx, 200, { "user_id": yin.param(ctx, "id") })
});
let api = yin.group(app, "/api");
yin.get(api, "/users", fn(ctx) { yin.json(ctx, 200, [...]) });
yin.run(app, "127.0.0.1:8080")
```

## Software Transactional Memory (STM)

`shared expr` creates a `SharedCell` wrapped in `Value::Shared`:

```rust
pub struct SharedCell {
    value: Value,
    version: u64,
}
```

`transact { ... }` evaluates its body in a `TransactionContext` that:

1. Takes a snapshot of all `Shared` cells read.
2. Buffers writes in a local map.
3. On commit, checks that all read cells still have the same version
   (snapshot isolation). If any changed, the transaction retries (max 64
  times).
4. `retry` explicitly re-runs the transaction.

Nested transactions commit to the parent's buffer.

## Error Handling

`InterpreterError` is the unified error type:

```rust
pub enum InterpreterError {
    RuntimeError { msg, span },
    TypeError { expected, got, op, span },
    ArityError { expected, got, callee, span },
    ImportError { path, msg, span },
    // ...
}
```

`raise expr` throws a user exception (any `Value`). `try/rescue` catches them.
Runtime errors (type errors, arity errors, etc.) are NOT caught by `rescue` —
they propagate as `Err(InterpreterError::...)` through the evaluator.

## Testing

Tests live in `tests/`:

| File | Tests | Coverage |
|------|-------|----------|
| `lexer_test.rs` | 28 | Lexer |
| `parser_test.rs` | 42 | Parser |
| `roundtrip_test.rs` | 6 | Parse→print→parse roundtrip |
| `interpreter_test.rs` | 73 | Core evaluator + SendValue + C3 |
| `higher_order_test.rs` | 40 | map/filter/fold/etc. |
| `loops_test.rs` | 31 | while/loop/break |
| `math_test.rs` | 42 | min/max/floor/sqrt/etc. |
| `string_test.rs` | 42 | len/split/join/etc. |
| `actor_test.rs` | 26 | Actor runtime |
| `transact_test.rs` | 24 | STM |
| `module_test.rs` | 17 | Module system |
| `stdlib_test.rs` | 49 | Standard library |
| `http_test.rs` | 17 | lib/http.1y |
| `yin_test.rs` | 19 | lib/yin.1y (routes, groups, middleware) |
| `parallel_test.rs` | 7 | parallel module (multi-threading) |
| **Total** | **502** | |

Coverage is not yet measured by `cargo-tarpaulin` on all platforms (Windows
support is incomplete). Manual coverage estimate based on test scope:
core evaluator >90%, stdlib ~80%, error paths ~60%.

## Performance (yin benchmark)

Benchmarked with `tests/bench_yin.py` against `examples/yin_bench_server.1y`
(release build, N-worker pool where N = CPU cores, Windows). The `/ping` route
returns a small JSON response; `/slow` awaits `process.sleep_async(500)`.

### Concurrency (GET /ping)

| N (requests) | Sequential (ms) | Concurrent (ms) | Seq (req/s) | Conc (req/s) | Speedup | Success |
|-------------:|----------------:|----------------:|------------:|-------------:|--------:|--------:|
| 10           |            64.9 |            33.8 |       154.0 |        296.0 |  1.92x  |  10/10  |
| 100          |           542.7 |           192.0 |       184.3 |        520.9 |  2.83x  | 100/100 |
| 1000         |          4947.6 |           865.4 |       202.1 |       1155.6 |  5.72x  | 1000/1000 |
| 10000        |         44245.3 |          6463.6 |       226.0 |       1547.1 |  6.85x  | 7689/10000 |

**Optimization history:**
- **Original** (sleep_ms polling): 10000 concurrent = 279 req/s, 1.6% success
- **accept_async** (mio event-driven): 10000 concurrent = 534 req/s, 99.96% success
- **Multi-threaded** (N-worker pool + batch accept): 10000 concurrent = 1547 req/s, 76.9% success

The multi-threaded version achieves **5.5x throughput improvement** over the
original at 10000 concurrent requests, and 1000 concurrent reaches 1156 req/s
with 100% success.

### Colorless async (slow handler does not block)

Sending 1 `GET /slow` (500ms await) + 5 `GET /ping` concurrently: all 5 fast
requests finish **before** the slow request, confirming that `await
process.sleep_async` in one handler does not block the event loop for other
handlers. This is the core property of 1y's colorless async.

To reproduce:

```
# Terminal 1
cargo run --release -- run examples/yin_bench_server.1y

# Terminal 2
python tests/bench_yin.py
```

## Multi-threading (`parallel` module)

`1y` provides user-facing multi-threading via the `parallel` module, built on
the `WorkerPool` (N worker threads, one per CPU core). Each worker pre-loads
the entry file's definitions (functions, actors, types, imports) and stays
alive to accept function calls.

### API

| Function | Signature | Description |
|----------|-----------|-------------|
| `parallel.cores` | `() -> Int` | Number of CPU cores |
| `parallel.call` | `(Str, Vec) -> Value` | Synchronously call a named function on a worker |
| `parallel.spawn` | `(Str, Vec) -> Handle` | Asynchronously call a function, return a handle |
| `parallel.join` | `(Handle) -> Value` | Wait for a spawned task and return its result |
| `parallel.map` | `(Str, Vec<Vec>) -> Vec` | Call a function in parallel for each arg set |

### Usage

```1y
fn heavy_compute(n) {
    let s = 0;
    let i = 0;
    while i < n { s = s + i; i = i + 1 };
    s
}

// Synchronous: blocks until result is ready
let r = parallel.call("heavy_compute", [1000000]);

// Asynchronous: returns immediately
let h1 = parallel.spawn("heavy_compute", [1000000]);
let h2 = parallel.spawn("heavy_compute", [2000000]);
let r1 = parallel.join(h1);
let r2 = parallel.join(h2);

// Parallel map: all tasks run concurrently
let results = parallel.map("heavy_compute", [[1000], [2000], [3000], [4000]]);
```

### Constraints

- Functions are called **by name** (string), not by closure reference. This is
  because `1y`'s `Value` is `Rc`-based (`!Send`); only `SendValue` (Int, Str,
  Bool, Nil, Vec, Map, Set, Variant, Struct) can cross thread boundaries.
- Arguments and return values must be `SendValue`-compatible. Functions,
  shared cells, actors, tasks, and opaque resources cannot be sent across
  threads.
- Worker threads pre-load definitions only (FuncDef, ActorDef, TypeDef,
  EnumDef, Import). Side-effect statements (Let, Expr) are skipped, so workers
  don't re-run main-program logic.

## Future: Bytecode VM

The current tree-walking interpreter is the primary performance bottleneck.
Each `1y` statement traverses the AST at runtime, incurring dispatch overhead
per node. A bytecode VM would compile the AST to a flat instruction sequence
once, then execute via a compact dispatch loop — typically 10-100x faster.

This is a long-term goal requiring:
- Bytecode instruction set design (opcodes for each AST node type)
- Compiler pass (AST → bytecode)
- Stack-based VM execution loop
- Integration with existing features (actors, STM, colorless async, modules)

The current architecture (AST nodes in `ast/mod.rs`, evaluator in
`interpreter/mod.rs`) is structured to allow a bytecode compiler to be added
as a separate pass without rewriting the type system or runtime.
