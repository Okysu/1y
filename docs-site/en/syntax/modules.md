---
title: Module System
---

# Module System

As a program grows, stuffing all the code into a single file is bad for both organization and reuse. 1y's module system lets you split code across multiple files and bring it in on demand. A module is both a **namespace** (gathering a group of related bindings under one name) and a **unit of reuse** (a file can export bindings for other files to use). 1y ships with a set of ready-to-use standard library modules and lets you organize your own code with the same mechanism.

## import Basics

An `import` statement brings a module into the current scope and binds it to a name. The most common form is `import name;`, where the bound name is the module name itself:

```1y
import io;

io.println("hello");          // call an exported function via io
let content = io.read_file("data.txt");
```

If you want to refer to a module by a different name (for example, to avoid a name clash or to fit your context better), use `import ... as alias`:

```1y
import io as fs;

fs.println("hello via fs");
fs.write_file("out.txt", "data");
```

The name after `as` becomes the module's alias in the current scope; the original name is no longer available. This is especially useful when importing several modules with similar functionality.

## lazy import: Deferred Loading

`lazy import` defers a module's loading until the **first time it is actually accessed**. When a module is expensive (for example, it performs heavy initialization) or is only needed on a few code paths, deferred loading avoids unnecessary startup cost:

```1y
lazy import json;

// json is not actually loaded yet

fn maybe_parse(s) {
    if looks_like_json(s) {
        json.parse(s)        // first access to json — the module loads here
    } else {
        nil
    }
}
```

The only semantic difference between `import` and `lazy import` is the loading time: once loaded, the two are identical in use. For the vast majority of lightweight modules, plain `import` is fine; `lazy import` is for modules that "might not be needed" or "are costly to load."

## Standard Library Modules

1y bundles a set of standard library modules covering common foundational capabilities. They use the same `import` mechanism as user-defined file modules and need no extra configuration:

| Module    | Purpose                                  |
|-----------|------------------------------------------|
| `env`     | Environment variables                    |
| `io`      | File I/O                                 |
| `json`    | JSON parse and stringify                 |
| `process` | Process control                          |
| `random`  | PRNG (xorshift64, NOT crypto-secure)     |
| `serial`  | Serial port I/O                          |
| `socket`  | TCP networking                           |
| `crypto`  | Hashing, HMAC, encoding, CSPRNG          |
| `tls`     | TLS client (rustls)                      |
| `ffi`     | Dynamic library loading                  |

```1y
import json;
import random;

let data = json.parse("{\"name\": \"Alice\"}");
let n = random.int(1, 100);
```

## File Modules

Beyond the standard library, you can organize your own `.1y` files as modules. Module paths are dot-separated: `a.b.c` resolves to `a/b/c.1y` **relative to the entry file's directory**.

Suppose your project is laid out like this:

```
main.1y
utils/
  math.1y
  strings.1y
```

In `main.1y`, you can import them like this:

```1y
import utils.math;
import utils.strings as str_utils;

println(utils.math.square(5));    // call square from utils/math.1y
println(str_utils.upper("hi"));   // call upper from strings.1y via alias
```

A `.1y` file's **top-level bindings** are exactly what it exports as a module. Everything you define at the top level with `let`, `fn`, `type`, or `enum` becomes a member of that module, accessible to importers via `module.member`. There is no explicit `export` keyword.

```1y
// utils/math.1y
fn square(x) -> Int { x * x }
fn cube(x) -> Int { x * x * x }
let PI = 3.14
```

Importers can then use `utils.math.square`, `utils.math.cube`, and `utils.math.PI`.

## Module Caching and Circular Dependencies

Modules are cached by their **canonical path**: each module file is loaded and evaluated at most once, and every place that imports it afterwards shares the same module instance. This guarantees consistency for any global state (such as mutable top-level bindings) and avoids the cost of reloading.

```1y
import utils.config;
import utils.config as cfg;   // same module — config and cfg reference one instance
```

The caching mechanism brings one constraint, however: **circular imports raise an error**. If module A imports B, and B (directly or indirectly) imports A, 1y raises an error when it detects the cycle, because no evaluation order can be determined between two mutually dependent modules. For example:

```1y
// a.1y
import b;        // b in turn imports a — a cycle!
```

```1y
// b.1y
import a;
```

When you hit a circular dependency, the right fix is to **refactor**: extract the parts both modules depend on into a third module, turning the dependency graph into a directed acyclic graph. `lazy import` can sometimes break a load-time cycle, but fundamentally module dependencies should be one-directional.

## Summary

`import` brings a module in and binds it to a name, `as` provides an alias, and `lazy import` defers loading until first access. Standard library modules work out of the box, file modules resolve as `a.b.c → a/b/c.1y`, and top-level bindings are the exports. Modules are cached by canonical path and loaded only once, and circular imports raise an error — together these rules keep the module system flexible yet predictable, so you can confidently split your program into clear, reusable parts.
