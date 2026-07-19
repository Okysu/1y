---
title: Bytecode VM
---

# Bytecode VM

Starting with the tree-walking interpreter, 1y now ships a stack-based bytecode virtual machine that compiles the AST into a flat instruction sequence and executes it in a dispatch loop. The VM is the default execution backend (`1y <file>`); the legacy tree-walker is still available via `1y run <file>` for debugging and comparison.

## Why a VM?

The original tree-walking interpreter evaluates the AST recursively. Each expression evaluation allocates a new `EnvRef` and walks the AST node-by-node through deeply nested `match` arms. This is simple but slow: ~5–10 KB of stack per recursion level and per-node dispatch overhead.

A bytecode VM addresses both issues:

- **Flat dispatch loop** — opcodes are decoded in a tight `match` in `VmCtx::step`, with no AST traversal.
- **Heap-allocated call frames** — the VM stack is a `Vec<Value>` on the heap, so deep recursion (e.g. `fib_memo(10000)`) no longer overflows the Rust stack.
- **Instruction reuse** — compiled closures share their `Chunk` (opcode buffer), so a function defined once is cheap to call many times.

## Architecture

```
┌─────────────────────────────┐
│            Vm               │  shared global state
│  ─ global env               │  (imports, type registry, live actors)
│  ─ variants / structs       │
│  ─ live_actors              │
│  ─ scheduler                │
│  ─ module cache             │
└────────────┬────────────────┘
             │ spawn per handler
             ▼
┌─────────────────────────────┐
│          VmCtx              │  per-coroutine execution context
│  ─ stack: Vec<Value>        │  operand + call stack
│  ─ frames: Vec<Frame>       │  call frames (return PC, locals base)
│  ─ open_upvalues: Vec<...>  │  Lua-style open upvalues
│  ─ chunks: Vec<Rc<Chunk>>   │  bytecode chunks (shared)
└─────────────────────────────┘
```

The split between `Vm` (shared, long-lived) and `VmCtx` (per-handler, short-lived) mirrors the actor model: each `Handle` message spawns a fresh `VmCtx` that runs to completion (or suspends) on the shared `Vm`'s scheduler.

### Compilation

`Compiler` walks the AST once per function and emits a `Chunk`:

```
fn fib(n) {
    if n < 2 { return n };
    fib(n - 1) + fib(n - 2)
}
```

compiles to roughly:

```
LoadLocal 0      ; push n
PushInt 2
Less
JumpIfFalse L1
LoadLocal 0      ; return n
Return
L1:
LoadGlobal "fib" ; fib(n - 1)
LoadLocal 0
PushInt 1
Sub
Call 1
LoadGlobal "fib" ; fib(n - 2)
LoadLocal 0
PushInt 2
Sub
Call 1
Add
Return
```

### Operand Stack & Call Frames

The VM is stack-based. Each `Call` pushes a new `Frame` recording the return PC and locals base; `Return` pops it. Locals are stored on the stack directly (no separate locals array), so `LoadLocal 0` is a single array index.

### Closures & Upvalues

Closures capture upvalues from enclosing scopes. Following Lua's design:

- **Open upvalue** — points into a stack slot of a still-active frame.
- **Closed upvalue** — the stack slot has been popped; the value is moved to the heap so the closure can still reach it.

When a closure escapes to another coroutine (e.g. via actor message `!`), all its open upvalues are closed eagerly by `close_escaping_upvalues`, preventing the receiver from reading garbage in its own (separate) stack.

## Supported Opcodes

The VM implements the full opcode set needed for 1y's core semantics:

| Category | Opcodes |
|----------|---------|
| Literals | `PushNil`, `PushInt`, `PushDecimal`, `PushStr`, `PushBool` |
| Variables | `LoadLocal`, `StoreLocal`, `LoadGlobal`, `StoreGlobal`, `LoadUpvalue`, `StoreUpvalue` |
| Collections | `NewVec`, `NewMap`, `GetField`, `SetField`, `GetIndex`, `SetIndex` |
| Control flow | `Jump`, `JumpIfFalse`, `JumpIfTrue`, `Return` |
| Calls | `Call`, `TailCall` (limited) |
| Functions | `Closure`, `PopLocalKeep` |
| Actors | `Spawn`, `ActorSend` (`!`), `ActorCall` (`?`), `ActorReply` |
| Async | `Await`, `Yield` |
| Modules | `Import`, `GetMember` |
| Pattern matching | `Match`, `TestTag`, `TestLiteral`, `Bind`, `JumpIfNoMatch` |
| Structs/Enums | `NewStruct`, `NewVariant`, `GetVariantArgs` |

## Async & the Scheduler

The VM reuses the same `Scheduler` as the tree-walker. `await` compiles to `OpCode::Await`:

- **Inside a coroutine** — `await_task` reads the thread-local `CURRENT_YIELDER` and suspends via `yielder.suspend()`. The scheduler parks the coroutine and runs others.
- **Top-level fallback** — when not in a coroutine (e.g. the HTTP accept loop's `await accept_async`), the VM busy-polls the Task but **drives the scheduler between polls**, so parked coroutines (timers, I/O) keep making progress instead of starving.

This is what makes the yin web server concurrent under load: the accept loop is a regular `loop { await accept_async; yield }`, and its top-level `await` interleaves scheduler ticks with its own polling, so slow handlers' timers fire while the accept loop waits for new connections.

See [Colorless Async](../syntax/async) for the user-facing API.

## Stack Overflow Recovery

The tree-walker's `fib_memo(10000)` used to overflow the 256 MB Rust stack because each recursion consumed ~10 KB. The VM solves this by moving call frames to the heap (`Vec<Frame>`), so the only Rust stack frames are the `VmCtx::step` dispatch loop itself — bounded regardless of 1y-side recursion depth.

Benchmark: `fib_memo(10000)` runs in the VM with no stack growth; `fib_memo(100000)` completes in ~1 second.

## Control Signals & Handler Stacks

The VM has 6 control signals: `Break` / `Continue` / `Retry` / `UserException` / `Reply` / `Return`. The first four need to find their corresponding handler (loop / transact / exception) before they can be consumed.

The original implementation matched handlers with `stack_depth >= cur_frame_base`, but this has a subtle hazard: a child frame's `stack_base` can equal the parent frame's handler `stack_depth`, causing the parent's handler to match incorrectly — `try { fn_that_raises() } rescue { ... }` would jump rescue to the wrong location, IP out of bounds and panic.

The fix adds a `frame_depth` field (`frames.len()`) to all three handler stacks and switches to exact matching `handler.frame_depth == cur_frame_depth`:

- [`ExceptionHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `try`/`rescue`/`ensure`
- [`TransactHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `transact`/`retry`
- [`LoopHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `for`/`break`/`continue`

All three signal-dispatch paths go through `VmCtx::handle_signal(err, propagate_depth)`, which returns `SignalOutcome::Continue` (signal consumed) or `SignalOutcome::Done(value)` (`Reply`/`Return` hit target).

## Dynamic Evaluation (eval)

The VM-side `eval(src)` is implemented in [`VmCtx::eval_src`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs). The flow:

1. Parse the source (`crate::parser::parse`)
2. Compile to a chunk via `compile_program_with_types` — this injects the VM's persistent `variant_table` / `struct_table`, so eval'd code recognizes outer `enum` variants and `type` constructors
3. Push a child `Frame` on the current `VmCtx` and run the chunk (sharing `vm.globals`)
4. Step until the child frame returns; signals are routed through `handle_signal(e, target_depth)` — `raise` can be caught by an outer `try`

**Persistent type tables** are the `variant_table: HashMap<String, usize>` and `struct_table: HashMap<String, ()>` fields on `Vm`. All three compile entry points (`run_source` / `load_module` / `eval_src`) share them, so `enum`/`type` definitions stay visible across `eval` and `import` boundaries.

See [Reflection & Dynamic Evaluation](../syntax/introspection).

## Features Supported by the VM

The VM is now feature-complete with the tree-walker — every 1y language feature is natively supported in the bytecode backend, with no fallback:

- **Control flow**: `if`/`match`/`while`/`loop`/`for`/`break`/`continue`/`return`
- **String interpolation**: `"x = {x}"` compiles to a `to_str` + `+` chain
- **Pattern matching**: literals, bindings, `Variant`/`Struct`/`Vec` destructuring, Or-patterns, guards
- **Closures & upvalues**: Lua-style open/closed upvalues; recursively closes escaping upvalues on actor send
- **Exceptions**: `try`/`rescue`/`ensure`/`raise` via `PushTry`/`PopTry`/`RescueMatch`/`EnsureExit` opcodes
- **Software transactional memory**: `transact`/`retry` via `PushTransact`/`TransactCommit` with conflict detection and retry
- **Actor concurrency**: `actor` definitions, `spawn`, `!` (send), `?` (request), `reply`, `yield`
- **Colorless async**: `await` on corosensei stackful coroutines; top-level await drives the scheduler via `drain_mailboxes_async`
- **Module system**: `import path [as alias] [lazy]`, loaded on demand and cached
- **Reflection & eval**: `ast_of` / `eval` / `type_of` / `instance_of` / `variant_name` / `variant_args` and other builtins

All 502 tests pass, covering every feature above. The tree-walker (`1y run`) is kept as a reference implementation for comparison and debugging.

## Trying It

```bash
# VM (default)
1y examples/fibonacci.1y

# Tree-walker (for comparison / debugging)
1y run examples/fibonacci.1y

# Run the Rust VM test suite (cargo integration tests)
cargo test

# Run the 1y-implemented (self-hosted) VM on a file
1y selfvm examples/phase1.1y

# Run the self-hosted VM test suites
1y selfvm bootstrap/test_parser.1y
1y selfvm bootstrap/test_compiler.1y
1y selfvm bootstrap/test_vm.1y
```

See [Self-Bootstrapping](#self-bootstrapping) below for the 1y-implemented
toolchain.

## Self-Bootstrapping

1y is fully self-bootstrapping: the lexer, parser, compiler, and VM are
themselves implemented in 1y under `bootstrap/`. The 5-phase path is
**complete**:

1. ✅ **tree-walker in 1y** (`bootstrap/interp.1y`) — proves self-interpretation is feasible.
2. ✅ **parser in 1y** (`bootstrap/parser.1y`) — hand-written recursive descent producing `Vec` / `Map` ASTs (the structure returned by `ast_of`).
3. ✅ **bytecode compiler in 1y** (`bootstrap/compiler.1y`) — compiles ASTs into `Vec<Int>` bytecode.
4. ✅ **VM interpreter loop in 1y** (`bootstrap/vm.1y`) — `match`-dispatched opcode handling.
5. ✅ **self-hosted end-to-end runner** (`bootstrap/selfvm.1y`) — `1y selfvm <file.1y>` lexes, parses, compiles, and executes 1y source using only 1y-implemented components.

The 1y-implemented VM is a tree-walker over bytecode (it runs on the Rust
tree-walker), so it is slower than the Rust VM — but it proves the language
can self-host. See [Reflection & Dynamic Evaluation](../syntax/introspection)
for the `ast_of` / `eval` foundation that made this possible.

## Implementation

- [src/compiler/mod.rs](https://github.com/Okysu/1y/blob/main/src/compiler/mod.rs) — AST → Chunk compiler
- [src/vm/vm.rs](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `Vm` + `VmCtx` execution engine
- [src/runtime/scheduler.rs](https://github.com/Okysu/1y/blob/main/src/runtime/scheduler.rs) — coroutine scheduler (shared with tree-walker)
