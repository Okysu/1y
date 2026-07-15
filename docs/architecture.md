# `1y` Architecture

This document describes the internal architecture of the `1y` interpreter
implementation as of Phase 4.6.

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
        ├── process.rs  # Process control
        ├── random.rs   # xorshift64 PRNG
        ├── serial.rs   # Serial port I/O
        ├── socket.rs   # TCP networking
        ├── crypto.rs   # Hashing, HMAC, CSPRNG
        ├── tls.rs      # TLS client (rustls)
        └── ffi.rs      # Dynamic library loading
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
}
```

- `global`: the root environment containing builtins.
- `std_modules`: pre-built standard library modules, keyed by name.
- `module_cache`: file modules keyed by canonical path (prevents reload).
- `module_load_stack`: tracks the current import chain for cycle detection.
- `entry_dir`: the directory of the entry-point file, used for resolving
  relative module paths.

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

## Actor Runtime

Actors are created with `spawn(initial_state) { body }`:

- Each actor gets its own OS thread with a large stack (64 MB).
- Messages are sent via `actor ! Message(args)` — they queue in the actor's
  mailbox.
- `receive { Pattern => handler }` blocks until a matching message arrives.
- `reply(value)` sends a reply to the caller (synchronous request/reply).
- `state` is bound in the actor's body and persists across `receive` calls.

Actors do NOT share memory; the only inter-actor communication is messages.
This avoids the need for locks in user code.

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
| `parser_test.rs` | 41 | Parser |
| `roundtrip_test.rs` | 6 | Parse→print→parse roundtrip |
| `interpreter_test.rs` | 51 | Core evaluator |
| `higher_order_test.rs` | 40 | map/filter/fold/etc. |
| `loops_test.rs` | 31 | while/loop/break |
| `math_test.rs` | 42 | min/max/floor/sqrt/etc. |
| `string_test.rs` | 42 | len/split/join/etc. |
| `actor_test.rs` | 21 | Actor runtime |
| `transact_test.rs` | 24 | STM |
| `module_test.rs` | 17 | Module system |
| `stdlib_test.rs` | 73 | Standard library |
| **Total** | **416** | |

Coverage is not yet measured by `cargo-tarpaulin` on all platforms (Windows
support is incomplete). Manual coverage estimate based on test scope:
core evaluator >90%, stdlib ~80%, error paths ~60%.
