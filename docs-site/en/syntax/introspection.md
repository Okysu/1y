---
title: Reflection & Dynamic Evaluation
---

# Reflection & Dynamic Evaluation

1y ships a set of **introspection builtins** plus **`eval(src)` dynamic evaluation** for inspecting and constructing values at runtime, parsing source strings into ASTs, and executing 1y code stored in strings. This is the foundation for bootstrapping (implementing a 1y interpreter in 1y itself), and makes REPLs, debuggers, and code generators straightforward.

## Type predicates

Every built-in type has a matching `is_*` predicate returning `Bool`:

| Function | Tests for |
|----------|-----------|
| `is_int(v)` | `Int` |
| `is_decimal(v)` | `Decimal` |
| `is_str(v)` | `Str` |
| `is_bool(v)` | `Bool` |
| `is_nil(v)` | `Nil` |
| `is_vec(v)` | `Vec` |
| `is_map(v)` | `Map` |
| `is_set(v)` | `Set` |
| `is_number(v)` | `Int` or `Decimal` |
| `is_func(v)` | any callable (closure / native) |
| `is_closure(v)` | user-defined closure or `fn` literal |

```1y
is_int(42)           // true
is_decimal(3.14)     // true
is_str("hi")         // true
is_vec([1, 2, 3])    // true
is_closure(println)  // false — println is a native function
is_closure(fn(x){x}) // true
is_func(println)     // true — is_func includes natives
```

## `type_of(v)` — type name

Returns the type name as a string. Matches the second argument to `instance_of` (see normalization below).

```1y
type_of(42)              // "Int"
type_of("hi")            // "String"
type_of([1, 2])          // "Vec"
type_of(fn(x){x})        // "Closure"
type_of(println)         // "Native"
type_of(Some(42))        // "Variant"
```

## `instance_of(v, type_name)` — type check

Checks whether `v` belongs to the given type. For `Variant` / `Struct` it compares the constructor name; for other types it compares the normalized type name.

**Name normalization**: `type_of` returns `"String"` but the predicate family uses `"Str"`; `instance_of` normalizes them internally so both `"Str"` and `"String"` recognize strings. Likewise `"Func"` ↔ `"Closure"`.

```1y
enum Option { Some(v), None }
let s = Some(42)

instance_of(s, "Some")       // true
instance_of(s, "Option")     // false — compares variant name, not enum name
instance_of(s, "None")       // false
instance_of(42, "Int")       // true
instance_of("hi", "Str")     // true
instance_of("hi", "String")  // true — normalized to the same name
instance_of(fn(x){x}, "Closure") // true
instance_of(fn(x){x}, "Func")    // true
```

## Collection / struct introspection

| Function | Input | Output |
|----------|-------|--------|
| `keys(map_or_struct)` | `Map` / `Struct` | `Vec` of field names |
| `values(map_or_struct)` | `Map` / `Struct` | `Vec` of field values |
| `fields(struct)` | `Struct` | `Map` (name → value) |
| `has_key(map, key)` | `Map` | `Bool` |
| `count(coll)` | `Vec` / `Map` / `Set` / `Str` | `Int` |

```1y
let m = { "a": 1, "b": 2 };
keys(m)     // ["a", "b"] (or ["b", "a"] — hash order)
values(m)   // [1, 2]
has_key(m, "a") // true

type Point = { x: Int, y: Int };
let p = Point({ x: 10, y: 20 });
keys(p)     // ["x", "y"]
values(p)   // [10, 20]
fields(p)   // { "x": 10, "y": 20 }
```

## Variant introspection

| Function | Input | Output |
|----------|-------|--------|
| `variant_name(v)` | `Variant` | constructor name `Str` |
| `variant_args(v)` | `Variant` | `Vec` of carried values |

```1y
enum Tree { Leaf, Node(v, l, r) }

let t = Node(42, Leaf, Leaf);
variant_name(t)    // "Node"
variant_args(t)    // [42, Leaf, Leaf]

let l = Leaf;
variant_name(l)    // "Leaf"
variant_args(l)    // []
```

## `ast_of(src)` — parse source to AST

Parses a 1y source string into an AST and returns it as nested `Map` values (each node has shape `{ "type": "NodeType", ...fields }`). On parse failure it returns a `ParseError` struct rather than raising — convenient for programmatic handling.

```1y
let ast = ast_of("let x = 1 + 2; fn add(a, b) \{ a + b \}");
get(ast, "type")           // "Program"
count(get(ast, "stmts"))   // 2

let s0 = get(get(ast, "stmts"), 0);
get(s0, "type")            // "Let"
get(s0, "name")            // "x"

let val = get(s0, "value");
get(val, "type")           // "BinOp"
get(val, "op")             // "+"
get(get(val, "lhs"), "value")  // 1
get(get(val, "rhs"), "value")  // 2

let s1 = get(get(ast, "stmts"), 1);
get(s1, "type")            // "FuncDef"
get(s1, "name")            // "add"
count(get(s1, "params"))   // 2
```

### Parse-error structure

```1y
let bad = ast_of("let x = ;");
// bad == {
//   "type": "ParseError",
//   "message": "unexpected `;` in expression",
//   "line": 1,
//   "col": 9,
// }

get(bad, "type")     // "ParseError"
get(bad, "message")  // "unexpected `;` in expression"
get(bad, "line")     // 1
get(bad, "col")      // 9
```

### AST node shapes

Common node structures:

| Node type | Key fields |
|-----------|-----------|
| `Program` | `stmts: [Stmt]` |
| `Let` | `name: Str`, `value: Expr`, `is_rec: Bool` |
| `FuncDef` | `name: Str`, `params: [Param]`, `body: Expr` |
| `Expr` / `Semi` | `expr: Expr` |
| `BinOp` | `op: Str`, `lhs: Expr`, `rhs: Expr` |
| `UnaryOp` | `op: Str`, `operand: Expr` |
| `If` | `cond: Expr`, `then: Expr`, `else: Option<Expr>` |
| `Call` | `callee: Expr`, `args: [Expr]` |
| `Ident` | `name: Str` |
| `Int` / `Decimal` / `Bool` | `value: <same type>` |
| `Str` | `value: Str` (single part) or `parts: [StrPart]` (with interpolation) |

> Full definitions: [`src/ast/to_value.rs`](https://github.com/Okysu/1y/blob/main/src/ast/to_value.rs).

## `eval(src)` — dynamic evaluation

Parses, compiles, and executes a string as a 1y program. **Shares the global environment and type table with the caller** — definitions made inside `eval` (e.g. `fn`, `let`) persist into the global scope, and `eval` can reference outer `enum` variants and `type` constructors.

Returns the value of the last **expression**; if the last statement is a definition (`let` / `fn`), returns `Nil`.

```1y
// Simple expression
eval("1 + 2 * 3")              // 7
eval("\"hello\" + \" world\"") // "hello world"

// Multi-statement program: last expression wins
eval("let x = 10; let y = 20; x + y")  // 30

// Definitions persist into globals
eval("fn sq(n) \{ n * n \}");
sq(5)                          // 25 — outer code can call it

// References outer global
let base = 100;
eval("base + 1")               // 101

// Closures
eval("fn mk(n) \{ fn(x) \{ x + n \} \}; mk(10)(5)")  // 15

// Nested eval
eval("eval(\"1 + 2\") + 1")    // 4
```

### Recognizing outer-scope types

`eval` sees outer `enum` variants and `type` constructors by default:

```1y
enum EvError { Bad(String), NoArgs, WithTwo(Int, Int) }
type EvPoint = \{ x: Int, y: Int \};

let v = eval("Bad(\"from eval\")");
variant_name(v)                // "Bad"
variant_args(v)                // ["from eval"]

let z = eval("NoArgs");
variant_name(z)                // "NoArgs"

let p = eval("EvPoint(\{ x: 10, y: 20 \})");
instance_of(p, "EvPoint")      // true
fields(p)                      // { "x": 10, "y": 20 }
```

### Exception propagation

`raise` inside `eval` propagates outward and can be caught by an outer `try` / `rescue`:

```1y
try {
    eval("raise(\"boom from eval\")");
} rescue as e {
    println("caught: " + str(e));  // caught: boom from eval
}

// Variant exceptions propagate too
enum Err { Bad(String) }
try {
    eval("raise(Bad(\"nested\"))");
} rescue as e {
    variant_name(e);    // "Bad"
    variant_args(e);    // ["nested"]
}
```

> **Note**: `try` / `rescue` catches only `UserException` raised via `raise`, **not** runtime errors like type errors or parse errors — this matches tree-walker semantics. Use `ast_of` first if you need to validate source before running it.

### Empty program

`eval("")` returns `Nil` without error.

## Limitations

Current known limits of `eval` and the reflection builtins:

- **No cross-`import` variant sharing**: variants defined in modules can be referenced in `eval` strings (the VM maintains a persistent type table), but variant comparison **across actor boundaries** relies on the value's `name` field with no namespace-conflict check.
- **`ast_of` output is a convention, not a stable API**: specific field names may shift as the grammar evolves; dispatch on `get(ast, "type")` when processing programmatically.
- **`eval` is not a sandbox**: eval'd code shares globals and filesystem access with the caller — do not use it to run untrusted source.

## Bootstrapping roadmap

These capabilities are the foundation for 1y's self-bootstrapping. The planned 5-phase path:

1. ✅ **tree-walker in 1y** (`bootstrap/interp.1y`) — implement a tree-walker for a 1y subset inside 1y itself, proving self-interpretation is feasible.
2. ⏳ **parser in 1y** — hand-written recursive descent parser producing `Vec` / `Map` ASTs (i.e. the structure returned by `ast_of`).
3. ⏳ **bytecode compiler in 1y** — compile ASTs into `Vec<Int>` bytecode.
4. ⏳ **VM interpreter loop in 1y** — `match`-dispatched opcode handling.
5. ⏳ **replace the Rust VM** — run the full existing test suite under the 1y-implemented VM.

Phase 1 is complete; phases 2–5 are pending. See [Bytecode VM](../philosophy/bytecode-vm).

## References

- [Standard Library](./stdlib) — index of all builtins
- [Bytecode VM](../philosophy/bytecode-vm) — VM implementation details, including `eval`'s execution model
- [`src/ast/to_value.rs`](https://github.com/Okysu/1y/blob/main/src/ast/to_value.rs) — AST → `Value` conversion code (the implementation of `ast_of`)
- `VmCtx::eval_src` in [`src/vm/vm.rs`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — the VM-side implementation of `eval`
