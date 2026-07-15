# 1y

[中文](./README.zh-CN.md) | **English**

> A streaming, concurrent, functional programming language implemented in Rust.

`1y` (pronounced "one-why") is a tree-walking interpreted language that
brings together persistent data structures, pattern matching, actor-based
concurrency, software transactional memory (STM), and a pragmatic module
system — all backed by arbitrary-precision arithmetic.

---

## Highlights

- **Arbitrary-precision numbers** — integers and decimals never overflow.
  `fact(500)` returns a 1135-digit number natively.
- **Persistent collections** — `Vec`, `Map`, `Set` are immutable and
  structurally shared, powered by the `im` crate.
- **Pattern matching** — literals, binders, wildcards, Or-patterns, vec/map/
  struct/variant destructuring, with guards.
- **Actor concurrency** — isolated state, message passing (`!` for
  fire-and-forget, `?` for request/reply), no shared mutable state between
  actors.
- **Software Transactional Memory** — `shared` cells + `transact` blocks
  with snapshot isolation, atomic commit, rollback, nesting, and `retry`.
- **Module system** — `import` standard libraries or your own `.1y` files;
  `lazy import` defers loading until first access; circular-import detection.
- **Standard library** — `io`, `json`, `env`, `process`, `random`,
  `socket` (TCP), `serial` (RS-232), `crypto` (SHA/HMAC/base64/CSPRNG),
  `tls` (rustls), `ffi` (dynamic library loading via `libloading`).
- **Exceptions** — `raise`, `try / rescue / ensure`, any value can be thrown.
- **String interpolation** — `"hello {name}!"`, triple-quoted multi-line
  strings, `\{` to escape literal braces.

---

## Quick Start

### Build

```bash
cargo build --release
# binary: target/release/1y
```

### Hello World

```bash
1y run -e 'println("Hello, World!")'
```

### Run a file

```bash
1y run examples/phase1.1y
```

### REPL

```bash
1y repl
```

---

## Language Tour

```1y
// Functions are first-class values.
fn fib(n) -> Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

// Arbitrary-precision integers — no overflow, ever.
let big = fact(500)

// Persistent collections.
let xs = [1, 2, 3]
let ys = push(xs, 4)      // xs is unchanged

// Pattern matching with guards and Or-patterns.
match opt {
    Some(x) if x > 0 => "positive",
    Some(_)          => "non-positive",
    None | Err(_)    => "nothing"
}

// Pipe operator for fluent chains.
let result = xs |> filter(fn(x) { x > 0 }) |> map(double) |> sum

// Actor with isolated state.
actor Counter {
    state count = 0
    on inc()        { count = count + 1 }
    on get() -> Int { reply(count) }
}
let c = spawn Counter()
c ! inc()
let n = c ? get()

// Software Transactional Memory.
shared counter = 0
transact {
    counter = counter + 1
}

// Modules — standard library and your own .1y files.
import io
import json
import utils.math as m       // loads <entry_dir>/utils/math.1y
lazy import heavy_lib        // loaded on first use
```

---

## Examples

| File | Demonstrates |
|------|-------------|
| `examples/phase1.1y` | Core language: functions, closures, collections, pattern matching |
| `examples/phase2.1y` | Actor runtime: spawn, state, fire-and-forget, KV store |
| `examples/phase3.1y` | STM: shared cells, transact, retry, nesting |
| `examples/phase4.1y` | Module system, stdlib (io/json/random/process), lazy import |
| `examples/phase4.5.1y` | Crypto (SHA/HMAC/base64), TLS client, FFI stubs |
| `examples/phase4.6.1y` | Real FFI: load shared libraries, call native functions |
| `examples/bench.1y` | Benchmark suite: fib, factorial, loops, collections, JSON |

---

## Tooling

### VSCode Extension

The `editor/` directory contains a VSCode extension (`onely-vscode`) providing:

- **Syntax highlighting** — TextMate grammar for `1y` source.
- **LSP diagnostics** — hybrid strategy: fast in-process TS lexer for
  immediate feedback + optional `1y parse -` subprocess for authoritative
  parser errors.
- **Context-aware completions** — keywords, builtins, module functions,
  user-defined symbols.
- **Hover documentation** — for keywords, builtins, and user-defined
  functions/variables/types.
- **Document symbols** — outline of `fn`, `let`, `enum`, `type`, `actor`,
  `on` declarations.
- **Inline suggestions** — ghost-text completions for common constructs.

Build and install:

```bash
cd editor
npm install
npm run package        # produces onely-vscode-0.1.0.vsix
code --install-extension onely-vscode-0.1.0.vsix
```

### Documentation Site

The `docs-site/` directory contains a VitePress documentation site with
bilingual (Chinese / English) content covering design philosophy, syntax
reference, and examples.

```bash
cd docs-site
npm install
npm run dev            # local preview at http://localhost:5173
npm run build          # static site in docs-site/.vitepress/dist
```

Online docs: https://okysu.github.io/1y/

---

## Project Structure

```
1y/
├── src/
│   ├── ast/            # AST definitions + spans
│   ├── lexer/          # Hand-written lexer
│   ├── parser/         # Recursive-descent parser
│   ├── interpreter/    # Tree-walking evaluator
│   │   ├── builtins.rs # Built-in functions
│   │   ├── env.rs      # Lexical environment
│   │   ├── ops.rs      # Operators
│   │   └── stdlib/     # Standard library modules
│   ├── value.rs        # Runtime value types
│   ├── main.rs         # CLI entry point
│   └── lib.rs          # Library API
├── tests/              # Integration tests (410 tests)
├── examples/           # Example programs
├── editor/             # VSCode extension
├── docs-site/          # VitePress documentation
├── docs/               # Language guide, stdlib reference, architecture
└── Cargo.toml
```

---

## Testing

```bash
cargo test              # run all 410 tests
cargo test -- --nocapture phase1   # run a specific suite
```

---

## License

Dual-licensed under MIT or Apache-2.0, at your option.
