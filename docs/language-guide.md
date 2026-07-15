# `1y` Language Guide

`1y` (pronounced "one-why") is a streaming, concurrent, functional programming
language implemented in Rust. It features persistent data structures, pattern
matching, actor-based concurrency, software transactional memory, a module
system, and a standard library covering I/O, networking, crypto, TLS, JSON, and
FFI.

This guide covers the language as of Phase 4.6.

## Table of Contents

1. [Hello, World](#hello-world)
2. [Lexical Structure](#lexical-structure)
3. [Types](#types)
4. [Expressions](#expressions)
5. [Statements](#statements)
6. [Pattern Matching](#pattern-matching)
7. [Functions and Closures](#functions-and-closures)
8. [Custom Types](#custom-types)
9. [Control Flow](#control-flow)
10. [Modules and Imports](#modules-and-imports)
11. [Actors](#actors)
12. [Shared State and Transactions](#shared-state-and-transactions)
13. [Standard Library](#standard-library)
14. [FFI](#ffi)

---

## Hello, World

```1y
println("Hello, World!")
```

Run with the CLI:

```
1y run hello.1y
```

## Lexical Structure

- **Comments**: `//` line comments, `/* ... */` block comments (nestable).
- **Identifiers**: start with a letter or `_`, followed by letters/digits/`_`.
- **Numbers**:
  - Integers: `42`, `1_000_000` (arbitrary precision via `num-bigint`).
  - Decimals: `3.14`, `0.5` (via `bigdecimal`).
  - Underscore separators allowed: `1_000`.
- **Strings**: double-quoted, with interpolation `"...{expr}..."`. Triple-quoted
  `"""..."""` for multi-line. Escape with `\n`, `\t`, `\\`, `\"`, etc. To include
  a literal `{` in a string, escape it as `\{`.
- **Keywords**: `let`, `fn`, `if`, `else`, `match`, `enum`, `type`, `try`,
  `rescue`, `raise`, `import`, `lazy`, `as`, `loop`, `while`, `break`,
  `spawn`, `on`, `receive`, `shared`, `transact`, `retry`, `return`, `reply`.

## Types

| Type        | Literal example            | Notes                                      |
|-------------|----------------------------|--------------------------------------------|
| Int         | `42`, `-1`                 | Arbitrary precision                        |
| Decimal     | `3.14`                     | Arbitrary precision                        |
| Str         | `"hello"`                  | UTF-8, with interpolation                  |
| Bool        | `true`, `false`            |                                            |
| Nil         | `nil`                      | Unit/empty                                 |
| Vec         | `[1, 2, 3]`                | Persistent (im)                            |
| Map         | `{"a": 1}`                 | Persistent, keys are Values                |
| Set         | `#{1, 2, 3}`               | Persistent                                 |
| Func        | `fn(x) { x }`              | Closures capture environment               |
| Native      | `println`                  | Built-in or stdlib function                |
| Variant     | `Some(42)`                 | Enum variant                               |
| Struct      | `Point({x: 1, y: 2})`      | Named product type                         |
| Actor       | `spawn ...`                | Actor handle                               |
| Shared      | `shared 0`                 | Transactional cell                         |
| Module      | `io` (after `import io`)  | Namespace of exports                       |
| Opaque      | `<tcp-stream>`             | Native resource handle                     |

## Expressions

- **Arithmetic**: `+ - * / %`, with `/` promoting to Decimal when not evenly
  divisible. Mixed Int/Decimal promotes to Decimal.
- **Comparison**: `< <= > >= == !=`.
- **Logic**: `and or not`, short-circuiting.
- **Pipe**: `x |> f` is `f(x)`; `x |> f(a)` is `f(a, x)`.
- **Indexing**: `v[i]`, `m[key]`.
- **Field access**: `s.x`, `m.key` (Map shorthand for `get(m, "key")`).
- **Method call**: `recv.method(args)` calls a function value retrieved from
  `recv`; the receiver is passed as the first argument. Exception: when `recv`
  is a Module, the receiver is NOT passed (module methods are standalone).
- **String interpolation**: `"hello {name}!"`.

## Statements

- `let x = expr;` — bind a value.
- `x = expr;` — reassign an existing variable.
- Compound assignment: `x += 1`, `-=`, `*=`, `/=`, `%=`.
- `fn name(params) -> Type { body }` — declare a function.
- `type Name = { field: Type, ... }` — declare a struct type.
- `enum Name { Variant, Variant(Type), ... }` — declare an enum.
- `import path;` — eager module import.
- `lazy import path;` — deferred module import (loaded on first use).
- `import path as alias;` — bind module under a different name.
- Expression statements: any expression followed by `;`.

## Pattern Matching

```1y
match value {
    Pattern => expr,
    Pattern if guard => expr,
    _ => default
}
```

Patterns:
- **Literals**: `1`, `"str"`, `true`, `nil`.
- **Variables**: `x` binds the value.
- **Wildcard**: `_` matches anything.
- **Or**: `1 | 2 | 3` matches any.
- **Vec**: `[a, b, c]`, `[first, ..rest]` (with rest).
- **Map**: `{"key": val}`.
- **Struct**: `Point { x: px, y: py }`.
- **Variant**: `Some(x)`, `None`.

## Functions and Closures

```1y
fn add(a, b) -> Int { a + b }
let double = fn(x) { x * 2 };
```

- Functions are first-class values.
- Closures capture their defining environment by reference.
- Lambda: `fn(params) { body }`.
- Typed: `fn(Int) -> Int` is a function type annotation.

## Custom Types

**Struct**:
```1y
type Point = { x: Int, y: Int };
let p = Point({ x: 3, y: 4 });
p.x  // 3
```

**Enum**:
```1y
enum Option { Some(Int), None }
match Some(42) {
    Some(x) => x,
    None => 0
}
```

## Control Flow

- `if cond { ... } else { ... }` — expression, returns value.
- `while cond { ... }` — loop, returns Nil.
- `loop { ... break value }` — infinite loop, break returns value.
- `break` / `break value` — exit a loop.
- `try { ... } rescue Pattern as name { ... }` — catch exceptions.
- `raise expr` — throw an exception (any Value).

## Modules and Imports

```1y
import io;              // bind as `io`
import io as fs;        // bind as `fs`
lazy import json;       // load on first use
```

- **Std modules**: `env`, `io`, `json`, `process`, `random`, `serial`,
  `socket`, `crypto`, `tls`, `ffi`.
- **File modules**: `a.b.c` resolves to `<entry_dir>/a/b/c.1y`.
- A `.1y` file's top-level bindings become its module exports.
- Modules are cached by canonical path; circular imports raise an error.

## Actors

```1y
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => reply(n),
            Get => reply(state)
        }
    }
};
counter ! Inc(5)
```

- `spawn(initial_state) { body }` creates an actor.
- `actor ! Message(args)` sends a message (fire-and-forget).
- `actor ! Message(args)` with `reply` in the handler returns a value.
- `receive { Pattern => handler, ... }` blocks for a message.
- Each actor has isolated state.

## Shared State and Transactions

```1y
let counter = shared 0;
transact {
    let v = counter + 1;
    counter = v;
    v
}
```

- `shared expr` creates a transactional cell.
- Reading the cell uses the ordinary variable name (e.g. `counter`).
- `cell = expr` writes within a transaction (same syntax as ordinary assignment).
- `transact { ... }` provides snapshot isolation, atomic commit, rollback.
- `retry` re-runs the transaction (max 64 attempts).
- Nesting is supported (inner transactions commit to the outer).

## Standard Library

See [stdlib reference](stdlib-reference.md) for full function listings.

| Module   | Purpose                                  |
|----------|------------------------------------------|
| env      | Environment variables                    |
| io       | File I/O                                 |
| json     | JSON parse/stringify                     |
| process  | Process control                          |
| random   | PRNG (xorshift64, NOT crypto-secure)     |
| serial   | Serial port I/O                          |
| socket   | TCP networking                           |
| crypto   | Hashing, HMAC, encoding, CSPRNG          |
| tls      | TLS client (rustls)                      |
| ffi      | Dynamic library loading                  |

## FFI

```1y
import ffi;
let lib = ffi.load("libc.so.6");   // or msvcrt.dll on Windows
let r = ffi.call(lib, "abs", "int(int)", [-42]);
ffi.unload(lib);
```

- Signature format: `"ret(arg1, arg2, ...)"`.
- ABI types: `void`, `int` (i64), `uint` (u64), `float` (f64), `str` (C string).
- Up to 6 arguments supported.
- **Safety**: FFI is inherently unsafe; only load trusted libraries.
