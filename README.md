# 1y

[中文](./README.zh-CN.md) | **English**

> A streaming, concurrent, functional programming language implemented in Rust.

`1y` (pronounced "one-why") is a language with **two execution backends** — a
stack-based bytecode VM (default) and a tree-walking interpreter — that
brings together persistent data structures, pattern matching, actor-based
concurrency, software transactional memory (STM), Zig-style colorless async,
and a pragmatic module system, all backed by arbitrary-precision arithmetic.
It also ships **runtime reflection** (`ast_of`, `eval`, type predicates)
and is **fully self-bootstrapping**: the bytecode VM, compiler, parser, and
lexer are themselves implemented in 1y under `bootstrap/`.

---

## Highlights

- **Two execution backends** — a bytecode VM (default, `1y <file>`) with
  heap-allocated call frames that survives `fib_memo(100000)` without stack
  growth, plus a tree-walker (`1y run <file>`) for debugging and comparison.
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
- **Colorless async (Zig-style)** — `await` works inside any function
  without `async` coloring; corosensei stackful coroutines drive the
  scheduler so `accept_async` and slow handlers progress concurrently.
- **Module system** — `import` standard libraries or your own `.1y` files;
  `lazy import` defers loading until first access; circular-import detection.
- **Reflection & dynamic evaluation** — `eval(src)` executes 1y source
  strings sharing the caller's globals and type table; `ast_of(src)`
  returns the parsed AST as data; `type_of` / `instance_of` /
  `variant_name` / `variant_args` / `keys` / `values` / `fields` round out
  the introspection surface.
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
1y -e 'println("Hello, World!")'          # VM (default)
1y run -e 'println("Hello, World!")'      # tree-walker
```

### Run a file

```bash
1y examples/phase1.1y                     # VM (default)
1y run examples/phase1.1y                 # tree-walker
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

// Reflection & dynamic evaluation.
let ast = ast_of("1 + 2");   // { "type": "Program", "stmts": [...] }
eval("let x = 10; x * 2");   // 20 — definitions persist into globals
enum EvError { Bad(String) }
let v = eval("Bad(\"boom\")");
variant_name(v);             // "Bad"
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
| `bootstrap/lexer.1y` | **Self-bootstrap**: 1y lexer written in 1y |
| `bootstrap/parser.1y` | **Self-bootstrap**: recursive-descent parser → AST |
| `bootstrap/compiler.1y` | **Self-bootstrap**: AST → bytecode compiler |
| `bootstrap/vm.1y` | **Self-bootstrap**: bytecode VM interpreter loop |
| `bootstrap/selfvm.1y` | **Self-bootstrap**: end-to-end runner (lex → parse → compile → VM) |
| `bootstrap/test_parser.1y` | Parser test suite (compares 1y output vs Rust `ast_of`) |
| `bootstrap/test_compiler.1y` | Bytecode compiler test suite |
| `bootstrap/test_vm.1y` | VM test suite (arithmetic, closures, match/try, etc.) |

---

## Self-Bootstrapping

1y hosts its own implementation. The reflection builtins (`ast_of`, `eval`,
type predicates) made this possible — the 1y toolchain is now implemented
in 1y itself under [`bootstrap/`](./bootstrap/). The 5-phase path is
**complete**:

1. **✅ tree-walker in 1y** — [`bootstrap/interp.1y`](./bootstrap/interp.1y)
   implements a tree-walker for a 1y subset inside 1y itself.
2. **✅ parser in 1y** — [`bootstrap/parser.1y`](./bootstrap/parser.1y) is a
   hand-written recursive descent parser producing `Vec` / `Map` ASTs (the
   structure returned by `ast_of`).
3. **✅ bytecode compiler in 1y** — [`bootstrap/compiler.1y`](./bootstrap/compiler.1y)
   compiles ASTs to `Vec<Int>` bytecode.
4. **✅ VM interpreter loop in 1y** — [`bootstrap/vm.1y`](./bootstrap/vm.1y)
   is a `match`-dispatched bytecode VM.
5. **✅ self-hosted VM** — [`bootstrap/selfvm.1y`](./bootstrap/selfvm.1y)
   ties them together: `1y selfvm <file.1y>` lexes, parses, compiles, and
   executes 1y source using only 1y-implemented components.

Run the self-hosted VM:

```bash
1y selfvm examples/phase1.1y     # 1y running 1y-implemented VM
1y selfvm bootstrap/test_vm.1y   # self-hosted VM test suite
```

See [Reflection & Dynamic Evaluation](https://okysu.github.io/1y/syntax/introspection)
in the docs for details.

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
│   ├── ast/            # AST definitions + spans + to_value (AST → Value)
│   ├── lexer/          # Hand-written lexer
│   ├── parser/         # Recursive-descent parser
│   ├── compiler/       # AST → bytecode Chunk compiler
│   ├── vm/             # Stack-based bytecode VM (Vm + VmCtx)
│   ├── runtime/        # Coroutine scheduler + cross-thread actor registry
│   ├── interpreter/    # Tree-walking evaluator
│   │   ├── builtins.rs # Built-in functions
│   │   ├── env.rs      # Lexical environment
│   │   ├── ops.rs      # Operators
│   │   └── stdlib/     # Standard library modules
│   ├── value.rs        # Runtime value types
│   ├── main.rs         # CLI entry point
│   └── lib.rs          # Library API
├── tests/              # Integration tests (502 tests)
├── examples/           # Example programs
├── bootstrap/          # Self-bootstrapping toolchain (lexer/parser/compiler/vm in 1y)
├── editor/             # VSCode extension
├── docs-site/          # VitePress documentation
├── docs/               # Language guide, stdlib reference, architecture
└── Cargo.toml
```

---

## Testing

```bash
cargo test              # run all 502 tests
cargo test -- --nocapture phase1   # run a specific suite
```

---

## License

Dual-licensed under MIT or Apache-2.0, at your option.
