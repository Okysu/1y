---
title: Type System
---

# Type System

1y is a dynamically typed language, but every value has a definite type at runtime. The type system is deliberately minimal: there are only two numeric types (`Int` and `Decimal`), only three collection types (`Vec`, `Map`, `Set`), plus functions, custom types, and concurrency primitives — together they constitute all of 1y's value types. This chapter introduces each one and explains why persistent collections are the cornerstone of 1y's immutability philosophy.

## Type Overview

The table below lists all of 1y's value types and their literal forms:

| Type | Literal example | Notes |
|------|-----------------|-------|
| `Int` | `42`, `-1`, `1_000_000` | Arbitrary-precision integer (`num-bigint`) |
| `Decimal` | `3.14`, `0.5e10` | Arbitrary-precision decimal (`bigdecimal`) |
| `Str` | `"hello"`, `"""multi-line"""` | UTF-8 string, supports interpolation |
| `Bool` | `true`, `false` | Boolean |
| `Nil` | `nil` | Unit / empty type |
| `Vec` | `[1, 2, 3]` | Persistent vector (`im::Vector`) |
| `Map` | `{"a": 1}` or `{x: 1}` | Persistent hash map, keys are any `Value` |
| `Set` | `#{1, 2, 3}` | Persistent hash set |
| `Func` | `fn(x) { x }` | Closure, captures its defining environment |
| `Native` | `println` | Built-in or standard-library function |
| `Variant` | `Some(42)` | Enum variant instance: name + args |
| `Struct` | `Point({x: 1, y: 2})` | Struct instance: name + fields map |
| `Actor` | return value of `spawn ...` | Actor handle |
| `Shared` | return value of `shared 0` | Transactional cell |
| `Module` | `io` (after `import io`) | Namespace of a module's exports |
| `Opaque` | `<tcp-stream>` | Native resource handle |
| `LazyImport` | a `lazy import` binding | Deferred-load placeholder |

## Numeric Types: Int and Decimal

1y has only two numeric types, and both are arbitrary precision. This means **integers never overflow** and decimals never lose precision due to binary floating-point — one of 1y's most important design decisions.

```1y
let big = 170141183460469231731687303715884105727;   // a 128-bit integer, no problem
let precise = 3.141592653589793238462643383279502884197;
let factorial_100 = factorial(100);   // a 158-digit integer
```

The two kinds of numbers auto-promote during arithmetic: division that does not divide evenly promotes to `Decimal`, and `Int + Decimal` also promotes to `Decimal`. See the promotion rules in the [Expressions](./expressions) chapter.

## Primitive Values: Str, Bool, Nil

### Str

Strings are UTF-8 encoded, enclosed in double or triple quotes, and support interpolation:

```1y
let name = "alice";
let greeting = "hello, {name}!";        // interpolation
let poem = """
Line one
Line two
""";                                     // multi-line
```

Strings can be concatenated with `+` and operated on by standard functions such as `len`, `split`, and `trim`. Because strings are internally reference-counted (`Rc<String>`), copying them is cheap.

### Bool and Nil

```1y
let ok = true;
let empty = nil;
```

`Bool` has only the two values `true` and `false` and participates in `and`/`or`/`not` logical operations. `Nil` represents "no value" and is commonly the return value of functions that produce no meaningful result (such as `println`).

## Persistent Collections

1y's three collection types — `Vec`, `Map`, and `Set` — are all **persistent**: "modifying" a collection does not mutate the original but returns a new collection that shares most of its structure with the old one. This is the core of 1y's immutability philosophy.

### Persistence and Structural Sharing

A traditional mutable collection modifies memory in place when an element is added; a persistent collection, by contrast, returns a new collection that **shares the unchanged internal nodes** with the old one. This means the cost of "copying" is close to O(1), not O(n).

```1y
let v = [1, 2, 3];
let v2 = push(v, 4);     // returns a new vector [1, 2, 3, 4]
// v is still [1, 2, 3] — the original is unchanged
println(count(v));        // 3
println(count(v2));        // 4
```

The benefit of immutability is that **aliasing problems are eliminated at the root**. You never worry that "another function might silently mutate my data." In concurrent settings, data can flow freely between Actors with no copying and no synchronization.

### Vec — Persistent Vector

```1y
let xs = [1, 2, 3];
let ys = push(xs, 4);         // [1, 2, 3, 4]
let first = xs[0];            // index access, 1
let combined = fold(ys, xs, fn(acc, x) { push(acc, x) });  // concatenation via fold
```

`Vec` is backed by `im::Vector`, an RRB-tree implementation offering near-O(log₃₂ n) random access and O(1) amortized append.

### Map — Persistent Map

A `Map`'s keys can be **any `Value`**, not just strings. The literal has two forms: string keys may omit quotes, while non-string keys require the `key: value` syntax:

```1y
let m = { x: 1, y: 2 };              // string keys may omit quotes
let m2 = assoc(m, "z", 3);           // add a key-value pair
let m3 = dissoc(m2, "x");             // remove a key
let v = get(m, "x");                  // 1
let also_v = m.x;                     // field access is shorthand for get(m, "x")
```

Note that `{}` is parsed as an empty block (`Nil`), not an empty `Map`. To construct an empty `Map`, you typically start with a placeholder entry and remove it with `dissoc`, or build it incrementally with `assoc`.

### Set — Persistent Set

```1y
let s = #{1, 2, 3};
println(count(s));                    // 3

// membership check via fold
let has = fold(s, false, fn(acc, x) { if x == 1 { true } else { acc } });
println(has);                         // true

for x in s {
    println(x)
}
```

`Set` is backed by `im::HashSet`, offering O(1) average membership checks. Element-level mutation is done by building a new set literal or by folding over the set.

## Functions: Func and Native

```1y
let double = fn(x) { x * 2 };         // Func closure
let result = double(5);                // 10
```

Functions in 1y are first-class values: they can be passed as arguments, returned from functions, and stored in collections. Closures capture their defining environment by reference, so inner functions can refer to outer variables. `Native` represents built-in functions (such as `println`) or standard-library functions; from the caller's perspective there is no difference — both `Func` and `Native` can be called and piped into.

## Custom Types: Variant and Struct

1y uses two declarations — `enum` and `type` — to define custom types, producing `Variant` and `Struct` values at runtime respectively.

### Variant (Enum Variant)

An `enum` declares a set of named constructors, each of which may carry zero or more arguments:

```1y
enum Shape {
    Circle(Int),
    Rect(Int, Int)
}

let c = Circle(5);        // a Variant value: name "Circle" + args [5]
let area = match c {
    Circle(r) => r * r,
    Rect(w, h) => w * h
};
```

A `Variant` consists of a name and an argument list at runtime, making it ideal for expressing sum types.

### Struct

A `type` declaration defines a struct type — a record of named fields:

```1y
type Point = { x: Int, y: Int }

let p = Point({ x: 3, y: 4 });
let px = p.x;             // field access, 3
p.x = 42;                  // field assignment, returns a new struct
```

A `Struct` value consists of a type name and a fields `Map`. Field access `p.x` and field assignment `p.x = ...` operate directly on this map. In pattern matching, structs can be destructured with the `Point { x: px, y: py }` form.

## Concurrency Types: Actor and Shared

```1y
let counter = spawn Counter();       // Actor handle
counter ! Inc(1);                     // fire-and-forget
let n = counter ? Get;                // synchronous request/reply

let cell = shared 0;                  // transactional cell
transact {
    cell = cell + 1                   // read/write within a transaction
};
```

`Actor` and `Shared` are the two value types of 1y's concurrency model, corresponding to "message passing" and "shared transactional memory" respectively. They are expanded upon in the concurrency chapters.

## Modules and Opaque Types

After `import`, the module name binds to a `Module` value whose fields are the module's exports:

```1y
import io;
println(str(io));        // <module io>
let content = io.read_to_string("file.txt");
```

`Opaque` is a native resource handle (such as a TCP connection or a dynamic-library handle), created by standard-library functions. To 1y code it is opaque — you can only pass it back to the corresponding native function; you cannot directly manipulate its internals.

## What's Next

Types define "what values exist," while expressions define "how to compute new values." The next chapter, [Expressions & Operators](./expressions), covers every form of computation in 1y — arithmetic, comparison, pipes, field access, and interpolation.
