---
title: Quick Start
---

# Quick Start

Welcome to 1y. 1y (pronounced "one-why") is a streaming, concurrent, functional programming language implemented in Rust. It features arbitrary-precision numbers, persistent data structures, pattern matching, Actor concurrency, software transactional memory (STM), a module system, and FFI. This chapter walks you from zero to running your first program: installing the compiler, using the CLI tools, and executing the bundled examples.

## Building from Source

1y is currently distributed as source. You will need the Rust toolchain (`rustc` and `cargo`) on your system. If you do not yet have Rust installed, follow the instructions at [rustup.rs](https://rustup.rs/) to install a stable toolchain.

Once `cargo` is available, clone the repository and build:

```sh
git clone https://github.com/onely/1y.git
cd 1y
cargo build --release
```

After the build completes, the executable lives at `target/release/1y` (`target\release\1y.exe` on Windows). It is a good idea to add that directory to your `PATH` so you can invoke `1y` from anywhere.

If you just want to verify the compiler works quickly, you can use a debug build instead of a release build:

```sh
cargo build            # produces target/debug/1y
```

Debug builds are faster to compile but run slower than release builds. For learning and experimentation, a debug build is perfectly adequate.

## Hello, World

Following tradition, our first program prints a single line. Create a file `hello.1y` containing:

```1y
println("Hello, World!")
```

Then run it with the CLI:

```sh
1y run hello.1y
```

You should see `Hello, World!` printed to the terminal. That is all there is to it — a 1y program begins executing from the first statement in the file. There is no `main` function and no boilerplate required.

## The Command-Line Interface

The 1y CLI provides three core subcommands corresponding to three purposes: running, inspecting the AST, and inspecting tokens. These are especially useful when you are debugging syntax or trying to understand the language internals.

### `1y run` — Execute a Program

The most common command. It reads a `.1y` file, parses it, evaluates it, and executes its statements:

```sh
1y run path/to/program.1y
```

Program output is produced by functions such as `println` and `print`. Any uncaught exception is printed to stderr as an error message, and the process exits with a non-zero status code.

### `1y parse` — Print the AST

Parses the source into an abstract syntax tree (AST) and prints it without executing. This is invaluable when you want to confirm how a particular piece of syntax is structured:

```sh
1y parse hello.1y
```

For example, `let x = 1 + 2;` produces a `Let` node containing a `Binary` expression. This is an excellent entry point for understanding 1y's syntactic structure.

### `1y tokens` — Print Lexical Tokens

Performs only lexical analysis, splitting the source into a stream of tokens and printing each one. When you need to troubleshoot whether a literal or operator is being recognized correctly, this command gives you an immediate answer:

```sh
1y tokens hello.1y
```

For instance, `"hello {name}"` is broken into a string-start token, an interpolation expression, and a string-end token, letting you see exactly how interpolation is handled by the lexer.

## The Structure of a .1y File

A typical 1y source file consists of a number of top-level statements, executed sequentially from top to bottom. Below is a complete example covering imports, bindings, function declarations, and expression statements:

```1y
// Import a standard library module
import io;

// Top-level binding: binds an immutable value
let greeting = "hello";

// Function declaration
fn add(a, b) {
    a + b
}

// Expression statement: call a function and print the result
println(greeting + ", sum = " + str(add(3, 4)));
```

A few key points:

- **No entry function**: the file itself is the entry point, starting from the first statement.
- **Top-level bindings are exports**: when this file is imported as a module, its top-level `let` and `fn` bindings become that module's exports.
- **Semicolons**: statements usually end with a semicolon; however, at the end of a block expression or at the end of the file, the semicolon may be omitted.
- **Immutability by default**: a `let`-bound value is immutable. To rebind it, use `=` to assign the same name again.

## Running the Example Programs

The repository's `examples/` directory contains a set of example programs organized by development phase, from `phase0` through `phase4.6`. Each phase demonstrates a specific set of language features. It is recommended to run them in order to see how 1y's capabilities stack up:

```sh
1y run examples/phase1.1y      // arbitrary-precision arithmetic, persistent collections, pattern matching, closures, exceptions, pipes
1y run examples/phase2.1y      // Actor model: spawn, message passing, state isolation, state machines
1y run examples/phase3.1y      // software transactional memory: shared, transact, rollback, retry, nested transactions
1y run examples/phase3.5.1y    // loops, compound assignment, higher-order functions, string and math functions
1y run examples/phase4.1y      // module system: import, alias, lazy import, and the standard library
1y run examples/phase4.5.1y    // crypto, tls, and ffi module overview
1y run examples/phase4.6.1y    // real FFI: loading a dynamic library, calling C functions
```

Take `phase1.1y` as an example: it showcases 1y's "frontend" capabilities — arbitrary-precision arithmetic (factorial of 100 without breaking a sweat), structural sharing in persistent collections, closures and higher-order functions, custom types (structs and enums), pattern matching with guards and Or-patterns, exception handling, and the pipe operator. Running it produces an annotated series of outputs, each section corresponding to a language feature.

## What's Next

You can now run 1y programs. The following chapters start from the lowest layer — the lexical structure — and progressively unpack 1y's syntax and type system. It is recommended to read them in order:

- [Lexical Structure](./lexical-structure): comments, identifiers, literals, and keywords.
- [Type System](./types): the complete type inventory, from `Int` to persistent collections.
- [Expressions & Operators](./expressions): arithmetic, pipes, field access, and interpolation.
- [Statements](./statements): bindings, function declarations, and imports.
