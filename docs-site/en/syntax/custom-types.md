---
title: Custom Types
---

# Custom Types

1y ships with a rich set of built-in data types — `Int`, `Decimal`, `Str`, `Bool`, `Vec`, `Map`, `Set`, and so on — but real-world programs often need to describe the shape of data in their own domain. 1y offers two ways to define custom types: **structs** pack several fields into a named product, while **enums** express "this value is one of several mutually exclusive cases" as a tagged union. Both work hand in hand with pattern matching, forming the foundation of data modeling in 1y.

## Structs: Declaring with `type`

The `type` keyword creates a **struct type**. Its syntax is `type Name = { field: Type, field: Type, ... }`, with fields separated by commas:

```1y
type Point = { x: Int, y: Int };
type User = { name: Str, age: Int };
type Rect = { width: Int, height: Int };
```

Declaring a type only defines a "shape." To create a value of that type, call the **constructor function** named after the type, passing a Map literal to fill in the fields:

```1y
let p = Point({ x: 3, y: 4 });
let u = User({ name: "Alice", age: 30 });
let r = Rect({ width: 10, height: 5 });
```

The constructor is written `Name({ field: value, ... })` — the type name followed by a field Map in braces. This design makes construction visually symmetric with destructuring, and makes it easy to interoperate with Map patterns.

## Field Access

Struct fields are accessed with the `.` operator:

```1y
let p = Point({ x: 3, y: 4 });
println(p.x);   // 3
println(p.y);   // 4

let u = User({ name: "Alice", age: 30 });
println("{u.name} is {u.age} years old");   // Alice is 30 years old
```

You can also read fields within expressions and involve them in computations, deriving new values from an existing struct:

```1y
fn area(r) -> Int { r.width * r.height }

let r = Rect({ width: 10, height: 5 });
println(area(r));   // 50
```

Because values in 1y are immutable by default, you do not "mutate" a field; instead, you construct a new struct to express the updated state. This matters especially in concurrent settings — no one can read a stale value and get back half-mutated data.

## Structs and Pattern Matching

The most powerful use of structs is destructuring them with `match`. The pattern `Point { x: px, y: py }` binds fields to the given variables, while a literal pattern like `Point { x: 0, y: 0 }` matches a specific value exactly:

```1y
type Point = { x: Int, y: Int };

fn describe(p) -> Str {
    match p {
        Point { x: 0, y: 0 } => "origin",
        Point { x: px, y: py } => "({px}, {py})"
    }
}

println(describe(Point({ x: 0, y: 0 })));   // origin
println(describe(Point({ x: 3, y: 4 })));   // (3, 4)
```

For more on pattern matching, see the [Pattern Matching](./pattern-matching) chapter.

## Enums: Declaring with `enum`

When a value can only be "one of several mutually exclusive cases," declare a **tagged union** with `enum`. Each case is called a **variant**:

```1y
enum Option { Some(Int), None }

enum Color {
    Red,
    Green,
    Blue
}

enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}
```

Variants can carry different numbers and types of arguments:

- **Unit variants (no arguments)**: such as `Red`, `None`, `Point` — they are complete values on their own.
- **Single-argument variants**: such as `Some(Int)`, `Circle(Int)` — they carry one value.
- **Multi-argument variants**: such as `Rect(Int, Int)` — they carry several values.

To construct an enum value, call the corresponding variant name:

```1y
let a = Some(42);
let b = None;
let c = Circle(5);
let d = Rect(3, 4);
let e = Red;
```

## Handling Enums with match

Enums and `match` are made for each other. `match` branches on the variant's tag and binds the values it carries:

```1y
enum Option { Some(Int), None }

fn unwrap_or(opt, default) -> Int {
    match opt {
        Some(x) => x,
        None => default
    }
}

println(unwrap_or(Some(42), 0));   // 42
println(unwrap_or(None, 0));       // 0
```

For multi-argument variants, the pattern lists a corresponding number of binding variables by position:

```1y
enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}

fn area(s) -> Int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
        Point => 0
    }
}

println(area(Circle(2)));     // 12
println(area(Rect(3, 4)));    // 12
println(area(Point));         // 0
```

Enums force you to **exhaustively** handle every variant — combined with a `_` catch-all or by listing them one by one, you can catch "I forgot to handle this case" at compile/run time. This is the key reason `enum + match` is safer than a pile of boolean flags and `if` chains.

## Summary

`type` defines structs — it packs several named fields into a product, constructs via `Name({...})`, accesses fields via `.`, and destructures via the `Name { ... }` pattern. `enum` defines tagged unions — it lists several mutually exclusive cases, constructs via `Variant(args)`, and handles them via `match` arms. Together, they let you model data in a way that stays close to your problem domain, and then — with the help of pattern matching — clearly map "the shape of the data" onto "what to do for each shape."
