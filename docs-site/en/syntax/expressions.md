---
title: Expressions & Operators
---

# Expressions & Operators

1y is an expression-oriented language: almost everything has a value. `if` is an expression, `match` is an expression, and blocks are expressions. This chapter covers every form of computation in 1y — from arithmetic to pipes, from indexing to field access — and the semantic details of each. Once you understand these rules, you will be able to read and write idiomatic 1y code.

## Arithmetic

1y provides five arithmetic operators: `+` `-` `*` `/` `%`. They work on both `Int` and `Decimal` and follow a set of automatic promotion rules.

### Basic Operations

```1y
let a = 7 + 2;        // 9
let b = 7 - 2;        // 5
let c = 7 * 2;        // 14
let d = 7 % 2;        // 1
```

### Division and Automatic Promotion

The behavior of division `/` deserves special attention: **when two integers divide evenly, the result stays `Int`; when they do not, the result is automatically promoted to `Decimal`.** This means you never lose the fractional part of a division due to integer semantics.

```1y
let e = 6 / 2;        // 3, Int (divides evenly)
let f = 7 / 2;        // 3.5, Decimal (does not divide evenly, auto-promoted)
let g = 1 / 3;        // 0.3333..., Decimal
```

### Mixed-Type Promotion

When `Int` and `Decimal` are combined in an operation, the `Int` is automatically promoted to `Decimal`, and the result is `Decimal`. You never need to convert types manually — the language makes the mathematically correct choice for you.

```1y
let h = 1 + 0.5;       // 1.5, Decimal
let i = 2 * 3.14;       // 6.28, Decimal
let j = 10 - 0.1;       // 9.9, Decimal
```

The core philosophy behind these rules is: **the programmer should never have to worry about the width of numeric types.** Whether you mix `Int` with `Decimal` or perform division, the language produces the mathematically correct result.

## Comparison

The comparison operators `<` `<=` `>` `>=` `==` `!=` return `Bool`. They work on numbers, strings, and booleans:

```1y
let lt = 3 < 5;          // true
let ge = 3.14 >= 3.14;   // true
let eq = "a" == "a";     // true
let ne = true != false;  // true
```

Numeric comparison across `Int` and `Decimal` also works correctly: `1 == 1.0` is `true`.

## Logic

Logical operations use the word keywords `and`, `or`, `not`, rather than the symbols `&&`, `||`, `!`. They all **short-circuit**:

```1y
let ok = true and false;        // false
let yes = true or false;        // true
let no = not true;              // false

// Short-circuit: the second operand is only evaluated when needed
let x = nil;
let safe = x != nil and x > 0;  // false, does not trigger a comparison error on nil
```

Short-circuit evaluation lets you write "check-then-access" chains safely, without extra `if` nesting.

## The Pipe Operator `|>`

The pipe is one of 1y's most expressive operators, making data-flow style programming natural. `x |> f` is equivalent to `f(x)` — it "pipes" the value on the left into the function on the right.

### Single-Argument Pipes

```1y
let double = fn(x) { x * 2 };
let inc = fn(x) { x + 1 };

// 5 |> double |> inc  ==  inc(double(5)) == 11
let result = 5 |> double |> inc;
```

A chained pipe reads like an assembly line: a value flows left-to-right through a series of transformations. This is mathematically equivalent to the nested call `inc(double(5))`, but vastly more readable.

### Pipes with Extra Arguments

When the function on the right needs extra arguments, write `x |> f(a)` — this is equivalent to `f(a, x)`, meaning **the piped value is appended to the end of the argument list**:

```1y
let add = fn(a, b) { a + b };
// 5 |> add(10)  ==  add(10, 5) == 15
let sum = 5 |> add(10);
```

This convention lets the pipe work seamlessly with multi-argument higher-order functions like `map`, `filter`, and `fold`:

```1y
let nums = [1, 2, 3, 4, 5, 6];
let pipeline = nums
    |> filter(fn(x) { x % 2 == 0 })
    |> map(fn(x) { x * 10 })
    |> fold(0, fn(a, b) { a + b });
// 120  (20 + 40 + 60)
```

## Indexing

Square brackets `[]` perform indexing on `Vec` and `Map`:

```1y
let xs = [10, 20, 30];
let first = xs[0];          // 10

let m = {"a": 1, "b": 2};
let val = m["a"];            // 1
```

For `Vec`, the index is an `Int`; for `Map`, the key can be any `Value`. Indexing also supports assignment form `xs[0] = 99`, which returns a modified new collection.

## Field Access

The dot `.` has two meanings depending on whether the left-hand side is a struct or a `Map`.

### Struct Fields

For a `Struct` value, `s.x` reads the field named `x`:

```1y
type Point = { x: Int, y: Int }
let p = Point({ x: 3, y: 4 });
let px = p.x;               // 3
p.x = 42;                    // assignment, returns a new struct
```

### Map Field Shorthand

For a `Map` value, `m.key` is shorthand for `get(m, "key")` — as long as the key is a string, you can use a dot instead of brackets:

```1y
let m = { name: "alice", age: 30 };
let name = m.name;          // equivalent to get(m, "name"), value is "alice"
let age = m.age;            // 30
```

This makes reading from a `Map` (or an object parsed from JSON) as natural as accessing a struct field.

## Method Calls

`recv.method(args)` is a form of syntactic sugar: it retrieves the function value named `method` from the receiver `recv` and then calls it. The crucial detail is **how the receiver is passed**, which depends on the type of `recv`.

### Value Receivers: The Receiver Is the First Argument

When `recv` is an ordinary value (a struct, `Vec`, `Map`, etc.), the receiver itself is passed as the **first argument** to the function:

```1y
// Equivalent to push(vec, 4)
let vec = [1, 2, 3];
vec.push(4);                 // returns [1, 2, 3, 4]

// Equivalent to get(m, "key")
let m = { key: "v" };
m.get("key");                // "v"
```

### Module Receivers: The Receiver Is Not Passed

**The exception**: when `recv` is a `Module`, the receiver is **not** passed — module methods are standalone functions, and the dot is merely namespace qualification:

```1y
import io;
// io.read_to_string("file") directly calls read_to_string("file")
// it does not pass `io` itself as an argument
let content = io.read_to_string("file.txt");
```

This distinction is important: a module is a container of functions, not the first argument to them. Keep this rule in mind and you will be able to correctly determine what any method call expands to.

## String Interpolation

Within a double-quoted or triple-quoted string, `{expr}` is evaluated and replaced by its string representation:

```1y
let name = "world";
let n = 42;
let msg = "hello, {name}! answer = {n}";   // hello, world! answer = 42

// Any expression is allowed inside the braces
let xs = [1, 2, 3];
let s = "sum = {fold(xs, 0, fn(a, b) { a + b })}";   // sum = 6
```

If you need a literal brace in a string, escape it with `\{` and `\}`. String concatenation with `+` is also possible, but interpolation is usually clearer:

```1y
let a = "hello" + " " + "world";    // concatenation
let b = "hello, {name}!";            // interpolation, preferred
```

## Assignment Is Not an Expression

Finally, an important point: in 1y, **assignment is a statement, not an expression.** `x = 5` cannot be used as part of a larger expression (for example, you cannot write `if (x = 5) { ... }`). This design eliminates the classic bug of accidentally writing `=` instead of `==`. Compound assignments like `+=` and `-=` are likewise statements. See the [Statements](./statements) chapter for details.

## What's Next

Expressions tell us "how to compute," while statements tell us "how to organize computation." The next chapter, [Statements](./statements), covers all statement forms: bindings, function declarations, type declarations, and imports.
