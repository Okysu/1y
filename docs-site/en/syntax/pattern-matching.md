---
title: Pattern Matching
---

# Pattern Matching

Pattern matching is one of the most central control structures in 1y. It lets you **branch on the structure of a value**, and — at the same time — decompose that value into smaller parts. This process is called **destructuring**. Where `if` branches on a boolean condition, `match` branches on shape: it does not ask "is this value true or false?" but "what does this value look like?"

Pattern matching is everywhere in 1y: handling enum variants, unpacking structs, inspecting the shape of a collection, and catching exceptions can all be expressed with `match`. Compared to deeply nested `if` chains, `match` keeps branching logic flat and readable, and it encourages you to handle every case explicitly.

## Basic Syntax

A `match` expression consists of several **arms**, each of the form `Pattern => expr`, separated by commas. 1y tries each pattern top to bottom and executes the expression of the first arm whose pattern matches; that expression's value becomes the value of the whole `match`.

```1y
match value {
    Pattern => expr,
    Pattern => expr,
    _ => default
}
```

`match` is itself an expression, so it has a value and can appear anywhere a value is expected — for example, bound directly to a variable:

```1y
let describe = match n {
    0 => "zero",
    1 => "one",
    _ => "many"
};
```

The final arm uses the wildcard `_`, which matches anything. When none of the more specific patterns above hit, `_` acts as a catch-all. If you do not cover every possible case and omit the `_` fallback, a non-matching value will raise an error at runtime, so it is wise to get into the habit of providing a `_` catch-all.

## Literal Patterns

The simplest patterns are literals. Integers, strings, booleans, and `nil` can all be used directly as patterns:

```1y
match answer {
    42 => "the answer",
    0 => "nothing",
    _ => "something else"
}

match name {
    "Alice" => "hi Alice",
    "Bob" => "hi Bob",
    _ => "stranger"
}

match flag {
    true => "on",
    false => "off"
}
```

A literal pattern performs an **equality comparison**: it matches when the value is equal to the literal.

## Variable Binding and Wildcards

A lowercase identifier in a pattern is a **variable binding** — it matches any value and binds that value to the variable for use on the right-hand side of the arm. The underscore `_` is the wildcard: it also matches anything, but binds nothing.

```1y
match point {
    Point { x: px, y: py } => px + py   // bind fields to px, py
}

match opt {
    Some(x) => x * 2,                    // bind the inner value to x
    None => 0
}

match anything {
    _ => "matched but ignored"           // ignore the concrete value
}
```

Variable bindings are often mixed with literals: literals require "equality," while variables "capture the rest."

## Or Patterns

When you want several patterns to share a single arm, combine them with `|` into an **or pattern**. If any of the sub-patterns matches, the whole or pattern matches:

```1y
match status {
    200 | 201 | 204 => "ok",
    400 | 404 => "client error",
    500 | 502 | 503 => "server error",
    _ => "unknown"
}

match c {
    "a" | "e" | "i" | "o" | "u" => "vowel",
    _ => "consonant"
}
```

This spares you from repeating the same arm expression for each value.

## Vec Patterns

A `Vec` can be destructured by position. Inside the square brackets you list a pattern for each position; the `..rest` syntax additionally collects the remaining elements into a new `Vec`:

```1y
match xs {
    [] => "empty",
    [x] => "one element: {x}",
    [a, b] => "two: {a} and {b}",
    [first, ..rest] => "first is {first}, rest has {Vec.len(rest)} items",
    _ => "something else"
}
```

`[first, ..rest]` is an extremely useful pattern: it matches a vector with **at least one element**, binds the first element to `first`, and binds the remaining elements to `rest`. Note that a Vec pattern only matches when the vector truly has at least as many elements as the pattern requires; otherwise 1y falls through to the next arm.

## Field Patterns (Bare Struct Form)

A brace pattern without a type name — `{field: subpattern, ...}` — matches a `Struct` value by its field names. Inside the braces you write `field: subpattern`; a field matches only when it exists **and** its subpattern also matches:

```1y
type Config = { host: Str, port: Int };
let config = Config({ host: "localhost", port: 8080 });

match config {
    {host: h, port: p} => "{h}:{p}",
    {host: h} => "host only: {h}",
    _ => "no host"
}
```

A bare field pattern does not require you to list every field — as long as the fields you do list all match, the rest are ignored. This makes it ideal for "pluck a few fields on demand" scenarios. It is the unnamed counterpart of the `TypeName { ... }` form covered next.

## Struct Patterns

Structs are destructured with the form `TypeName { field: pattern, ... }`. To the left of each colon is a field name; to the right is a sub-pattern for that field's value:

```1y
type Point = { x: Int, y: Int };

let p = Point({ x: 3, y: 4 });

match p {
    Point { x: 0, y: 0 } => "origin",
    Point { x: px, y: py } => "point at ({px}, {py})"
}
```

Here `Point { x: 0, y: 0 }` uses literals to match the origin exactly, while `Point { x: px, y: py }` binds the fields to variables. As with bare field patterns, a Struct pattern only checks the fields you list; unlisted fields do not participate in matching.

## Variant Patterns (Enum Destructuring)

Enum variants are the classic use case for pattern matching. The `Variant(args)` pattern matches a specific variant and binds the values it carries:

```1y
enum Option { Some(Int), None }

match Some(42) {
    Some(x) => x,
    None => 0
}

enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}

match shape {
    Circle(r) => 3 * r * r,
    Rect(w, h) => w * h,
    Point => 0
}
```

`Point` is a unit variant (no arguments), and is itself a complete pattern. Variants that carry arguments require a corresponding number of sub-patterns.

## Guards

Sometimes structure alone is not enough to express a condition. By appending `if guard` to a pattern, you attach a boolean expression as a **guard**: the arm is selected only when the pattern matches **and** the guard is true.

```1y
match n {
    x if x > 0 => "positive",
    x if x < 0 => "negative",
    _ => "zero"
}

match opt {
    Some(x) if x > 100 => "big",
    Some(x) => x,
    None => 0
}
```

A guard may reference variables bound by the pattern (such as `x` above) as well as any variables in the enclosing scope. If a guard evaluates to false, 1y does not error — it **continues to the next arm**. This is important: it makes a guard an additional filter, not a hard assertion.

## Combining and Nesting Patterns

All patterns can be nested arbitrarily. You can put a Variant inside a Vec, a Struct inside a Variant, and wrap everything in or patterns and guards:

```1y
match request {
    [Some(cmd), ..rest] if cmd == "quit" => "bye",
    [Some(cmd), ..rest] => "run {cmd}",
    [None] => "empty request",
    _ => "malformed"
}

match event {
    Point { x: 0, y: 0 } | Point { x: px, y: py } if px == py => "diagonal or origin",
    Point { x: px, y: py } => "({px}, {py})"
}
```

Nested patterns let `match` describe the shape of complex data precisely, without first tearing the data apart and then inspecting each layer with `if`.

## Summary

Pattern matching unifies branching and destructuring. With this set of pattern primitives — literals, variables, wildcards, or, Vec, field, Struct, and Variant — together with guards, you can describe in a declarative way "what data I expect, and what to do when I see it." When you find yourself writing a long chain of `if`s to inspect a value's type and structure, it is usually a sign that they should be rewritten as a single `match`.
