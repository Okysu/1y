---
title: Statements
---

# Statements

1y is expression-oriented — most constructs produce a value. Yet programs still need a layer of "statements" to organize bindings, declarations, and imports. This chapter covers every statement form in 1y: from the most basic `let` binding, to function declarations, custom-type declarations, and module imports. Once you understand the semantics of each, you will be able to compose expressions into complete, runnable programs.

## `let` Bindings

`let` creates a **new** variable binding. Bindings are immutable by default — once bound, the name refers to a fixed value within the current scope:

```1y
let x = 42;
let name = "alice";
let xs = [1, 2, 3];
```

`let` introduces a new name in the current scope. If a variable with the same name already exists in an outer scope, `let` **shadows** it within the current scope rather than modifying the outer variable:

```1y
let n = 1;
{
    let n = 99;        // shadows the outer n, only in this block
    println(n);        // 99
};
println(n);            // 1, the outer n is unaffected
```

## Reassignment with `=`

Unlike `let`, `=` **reassigns an existing variable**. It does not create a new binding; it modifies the variable of the same name that already exists in the current scope:

```1y
let x = 1;
x = 99;                 // reassignment, not a let
println(x);             // 99
```

### The Difference Between `let` and `=`

This is the point most easily confused by newcomers, so commit it to memory:

- **`let x = expr;`** — declares a **new** variable. If a variable of the same name already exists, it shadows it.
- **`x = expr;`** — reassigns an **already-existing** variable. If the name has not yet been declared, it is an error.

```1y
let a = 1;       // declare a new variable a
a = 2;            // reassign the existing variable a
let a = 3;        // declare again, shadowing the previous a
```

Since values are immutable by default, `=` reassignment is essentially "make this name point to another value," not an in-place memory mutation. This is especially important for persistent collections.

## Compound Assignment

For convenience, 1y provides five compound assignment operators, all of which are shorthand for "read — operate — reassign":

```1y
let n = 10;
n += 5;            // equivalent to n = n + 5    → 15
n -= 3;            // equivalent to n = n - 3    → 12
n *= 2;            // equivalent to n = n * 2    → 24
n /= 4;            // equivalent to n = n / 4    → 6
n %= 4;            // equivalent to n = n % 4    → 2
```

Compound assignment also works on indexed and field positions of collections:

```1y
let xs = [1, 2, 3];
xs[0] += 10;        // xs[0] goes from 1 to 11

let m = { count: 0 };
m.count += 1;       // count goes from 0 to 1
```

## Function Declaration with `fn`

`fn` declares a function and binds it to a given name. Functions are first-class values — they can be passed as arguments, returned, and stored in collections:

```1y
fn add(a, b) {
    a + b
}

fn factorial(n) -> Int {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}
```

A function body is a block expression, and the value of its final expression is the return value — **no `return` keyword is needed** (although 1y supports `return`, in most cases you will not need it).

### Optional Type Annotations

Parameters and return types may carry type annotations, but these currently serve documentation purposes and do not affect runtime behavior:

```1y
fn greet(name: Str) -> Str {
    "hello, " + name
}
```

### Anonymous Functions (Lambdas)

In addition to named functions, you can write an anonymous function directly with `fn(params) { body }`, bind it to a `let`, or pass it as an argument:

```1y
let double = fn(x) { x * 2 };
let add = fn(a, b) { a + b };

// As an argument to a higher-order function
let xs = [1, 2, 3];
let squares = map(xs, fn(x) { x * x });
```

Closures capture their defining environment by reference, so inner functions can refer to outer variables:

```1y
fn make_adder(x) {
    fn(y) { x + y }       // captures the outer x
}

let add10 = make_adder(10);
add10(5);                  // 15
```

## Struct Type Declaration with `type`

A `type` declaration defines a struct type — a record of named fields:

```1y
type Point = { x: Int, y: Int };

let p = Point({ x: 3, y: 4 });
println(p.x);              // 3
```

After declaration, the type name `Point` itself acts as a constructor: `Point({...})` creates a `Struct` value. Fields are accessed via dot notation `p.x` and assigned via `p.x = ...`. Structs can also be destructured in `match` — see [Pattern Matching](./pattern-matching).

## Enum Declaration with `enum`

An `enum` declares a set of named constructors (variants), each of which may carry zero or more arguments. It is used to express sum types:

```1y
enum Shape {
    Circle(Int),
    Rect(Int, Int)
}

enum Result {
    Ok(Int),
    Err(String)
}
```

After declaration, `Circle`, `Rect`, `Ok`, and `Err` are available constructors; calling them produces `Variant` values:

```1y
let c = Circle(5);
let r = Rect(3, 4);
let ok = Ok(42);

let area = match c {
    Circle(r) => r * r,
    Rect(w, h) => w * h
};
```

A variant with no arguments (such as `None`) is itself a value and needs no call. Enums combined with pattern matching are 1y's primary means of expressing branching logic.

## Module Imports with `import`

`import` binds a module into the current scope. 1y supports three import forms:

```1y
import io;                  // default bind name = last segment of the path
import io as fs;            // alias: bound under the name fs
lazy import heavy_lib;      // deferred: loaded only when first used
```

### Eager Import and Aliases

`import io;` loads the `io` module eagerly and binds it under the name `io`. The `as` keyword changes the bound name:

```1y
import io as fs;            // afterwards call fs.read_to_string(...)
import process as proc;     // afterwards call proc.pid()
```

### Lazy Import

A `lazy import` binds only a placeholder; the module is not actually loaded until it is **first evaluated**. This is useful for optional dependencies or when you want to speed up startup:

```1y
lazy import random as rnd;
// No loading happens until rnd is accessed here
println(rnd.int(10));
```

### Module Path Resolution

- **Standard library modules**: `io`, `env`, `json`, `process`, `random`, `socket`, `crypto`, `tls`, `serial`, `ffi`.
- **File modules**: the path `a.b.c` resolves to `<entry_dir>/a/b/c.1y`. A `.1y` file's top-level bindings are its exports.
- Modules are cached by canonical path; circular imports raise an error.

## Expression Statements

Any expression followed by a semicolon `;` forms a statement — these are typically used for their side effects (such as printing), discarding the resulting value:

```1y
println("hello");           // call println, ignore its return value (nil)
x + 1;                       // evaluated but the result is discarded (rare, usually pointless)
counter ! Inc(1);           // send a message to an Actor
```

A block expression `{ ... }` can also be used as a statement; the value of its last expression is the value of the block. This means any piece of code can be extracted into a block without changing its semantics:

```1y
let result = {
    let a = 10;
    let b = 20;
    a + b
};                          // result is 30
```

## What's Next

At this point you have covered 1y's lexical structure, types, expressions, and statements — the "static" part of the language is complete. The core-features chapters that follow move on to more advanced topics: [Pattern Matching](./pattern-matching), [Functions & Closures](./functions), [Custom Types](./custom-types), and [Control Flow](./control-flow).
