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

## What's Not Yet in the VM

A handful of 1y features are still tree-walker-only (the VM falls back to the tree-walker or raises a clear error):

- `for` loops (compile-time stub)
- `break` / `continue`
- String interpolation
- `try` / `transact` (in progress)
- `actor` definitions (actor *spawn* and message passing work; actor body compilation is partial)

For maximum compatibility, the tree-walker (`1y run`) remains fully featured and is the reference implementation.

## Trying It

```bash
# VM (default)
1y examples/fibonacci.1y

# Tree-walker (for comparison / debugging)
1y run examples/fibonacci.1y

# Run the VM test suite
1y vm tests/vm_test.1y
```

## Implementation

- [src/compiler/mod.rs](https://github.com/Okysu/1y/blob/main/src/compiler/mod.rs) — AST → Chunk compiler
- [src/vm/vm.rs](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `Vm` + `VmCtx` execution engine
- [src/runtime/scheduler.rs](https://github.com/Okysu/1y/blob/main/src/runtime/scheduler.rs) — coroutine scheduler (shared with tree-walker)
