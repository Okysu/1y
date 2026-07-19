---
title: Standard Library
---

# Standard Library

1y ships with **10 standard library modules** covering environment variables, file I/O, JSON, process control, random numbers, serial ports, TCP networking, cryptography, TLS, and dynamic library loading. All modules are brought in with `import` and expose their functions under a namespace. This page gives each module's purpose and key functions so you can quickly find what you need; consult the language reference for full per-function signatures.

## Module Overview

| Module | Purpose | Key functions |
|--------|---------|---------------|
| `env` | Environment variables | `get`, `set`, `unset`, `args`, `vars` |
| `io` | File I/O | `read_line`, `read_to_string`, `write`, `append`, `exists` |
| `json` | JSON codec | `parse`, `stringify`, `pretty` |
| `process` | Process control | `exit`, `exec`, `exec_status`, `pid`, `cwd`, `set_cwd`, `sleep_ms`, `sleep_async` |
| `random` | PRNG (xorshift64) | `int`, `range`, `float`, `bool`, `pick`, `shuffle`, `seed` |
| `serial` | Serial port | `open`, `list`, `read`, `write`, `close` |
| `socket` | TCP networking | `listen`, `accept`, `connect`, `read`, `read_line`, `write`, `close`, `set_nonblocking`, `peer_addr`, `read_async` |
| `crypto` | Hashing / CSPRNG | `sha256`, `sha512`, `sha1`, `md5`, `hmac_sha256`, `hmac_sha512`, `base64_encode/decode`, `hex_encode/decode`, `random_bytes`, `secure_int`, `secure_float` |
| `tls` | TLS client (rustls) | `connect`, `read`, `read_line`, `write`, `close`, `peer_addr` |
| `ffi` | Dynamic library loading | `load`, `call`, `unload`, `is_loaded` |

> **Task combinators** (`task_all`, `task_any`, `task_ready`) are global built-in functions, not part of a module — see [Tasks](#tasks).

## env — Environment Variables

`env` reads and modifies the process's environment variables and accesses command-line arguments.

```1y
import env;

let home = env.get("HOME");
env.set("MODE", "debug");
env.unset("TEMP");

let argv = env.args();        // argument vector from startup
let all = env.vars();         // all current environment variables
```

`args` returns the startup argument vector and `vars` returns all environment variables, handy for configuration discovery and diagnostics. `set`/`unset` affect only the current process and its children — they never write back to the parent shell.

## io — File I/O

`io` provides text- and byte-level file reading and writing, plus existence checks.

```1y
import io;

let text = io.read_to_string("config.txt");
io.write("log.txt", "started");
io.append("log.txt", "step two done");

if io.exists("data.bin") {
    let line = io.read_line();   // read a line from standard input
    print("-" + line);
}
```

`read_line` reads a line from standard input; `read_to_string` reads an entire file into a string at once. `write` overwrites and `append` appends. When a file is missing, `exists` returns `false` whereas `read_to_string` errors — probe with `exists` first, or catch the error with `match`/`try`.

## json — JSON Codec

`json` converts bidirectionally between 1y values and JSON text.

```1y
import json;

let obj = { name: "1y", version: 1 };
let compact = json.stringify(obj);
let pretty = json.pretty(obj);   // indented, for human reading

let parsed = json.parse(compact);
```

`parse` turns JSON text back into 1y values (objects become Maps, arrays become Vecs, numbers keep the unified numeric type); `stringify` produces compact text and `pretty` produces indented, readable text. `parse` errors on malformed JSON, so wrap it in error handling.

## process — Process Control

`process` manages the current process: exiting, running subcommands, querying identity, changing directory, and sleeping.

```1y
import process;

let code = process.exec_status("ls", ["-la"]);
process.exec("echo", ["hello"]);

process.set_cwd("/tmp");
let dir = process.cwd();
let me = process.pid();

process.sleep_ms(500);     // sleep for 500 milliseconds
process.exit(0);
```

`exec` runs a subcommand and blocks until it finishes; `exec_status` returns the exit code; `exit` terminates the current process immediately. `sleep_ms` pauses the current execution flow for the given milliseconds — under the event-loop model it yields the scheduler, giving other Actors a chance to run. `sleep_async(ms)` returns a `Task<Nil>` that completes after `ms` milliseconds; `await process.sleep_async(ms)` suspends the current coroutine without blocking other Actors (this is the colorless-async primitive used to keep slow routes from stalling the event loop).

## random — Pseudo-Random Numbers

`random` is based on the xorshift64 algorithm and provides fast but **not cryptographically secure** pseudo-random numbers.

```1y
import random;

random.seed(42);
let n = random.int(100);            // [0, 100)
let r = random.range(1, 100);      // [1, 100)
let f = random.float();            // [0.0, 1.0)
let b = random.bool();
let choice = random.pick([1, 2, 3]);
let shuffled = random.shuffle([1, 2, 3, 4]);
```

For cryptographically secure randomness, use the `crypto` module's `random_bytes` / `secure_int` / `secure_float` instead. `seed` fixes the sequence for reproducible tests; if you never call `seed`, a default seed is used.

## serial — Serial Port

`serial` interacts with serial devices, common in embedded and industrial settings.

```1y
import serial;

let ports = serial.list();                 // enumerate available ports
let dev = serial.open("COM3", 115200);     // or /dev/ttyUSB0
serial.write(dev, "PING");
let data = serial.read(dev, 64);
serial.close(dev);
```

`open` takes a device name and baud rate and returns a port handle; the second argument to `read` is the maximum number of bytes to read. Serial communication is usually blocking, so use it carefully within the event loop.

## socket — TCP Networking

`socket` provides blocking TCP server and client capabilities and can switch to non-blocking mode.

```1y
import socket;

let listener = socket.listen("127.0.0.1", 8080);
let conn = socket.accept(listener);
socket.set_nonblocking(conn, true);
let line = socket.read_line(conn);
socket.write(conn, "HTTP/1.1 200 OK");
print(socket.peer_addr(conn));
socket.close(conn);

// client
let c = socket.connect("example.com", 80);
socket.write(c, "GET / HTTP/1.0");
socket.close(c);
```

`listen` creates a listening socket and `accept` blocks waiting for and returns a new connection. `set_nonblocking` makes subsequent `read`/`read_line` return immediately when no data is available rather than hanging, which pairs well with an event loop for concurrent serving. `peer_addr` returns the remote address for logging and authorization. `read_async(stream, n)` returns a `Task<Str|Nil>` that completes when up to `n` bytes are available on the stream; `await socket.read_async(stream, 65536)` suspends the coroutine until the OS reports the stream readable (via `mio`), so one slow connection never blocks the others.

## Tasks — Async Composition

`await` is the core suspension primitive (see [Colorless async](../philosophy/no-async.md)). A `Task` is a value produced by async I/O functions (`socket.read_async`, `process.sleep_async`) or by the global combinators below. Tasks are **single-use**: `await` consumes one. You can `await` from any function body — no `async` marker is needed.

| Function | Signature | Description |
|----------|-----------|-------------|
| `task_ready` | `task_ready(value) -> Task` | A Task that is already complete with `value`. |
| `task_all` | `task_all([t1, t2, ...]) -> Task<Vec<value>>` | Completes when **all** inputs complete; results are in order. Consumes all inputs on success. |
| `task_any` | `task_any([t1, T2, ...]) -> Task<value>` | Completes when **any** input completes; yields the first ready value. Consumes only the winner. |

```1y
import process;

// Wrap a plain value into a Task
let now = await task_ready(42);

// Run two sleeps concurrently and wait for both
let both = await task_all([
    process.sleep_async(100),
    process.sleep_async(150)
]);
println(str(count(both)));    // 2

// Race two Tasks; the faster one wins
let winner = await task_any([
    process.sleep_async(100),
    process.sleep_async(500)
]);
```

For long-lived concurrent state (a counter, a cache, a session) prefer **Actors** (`spawn Name(args)`); `Task` is for composing async I/O, not for shared mutable state.

## crypto — Hashing and CSPRNG

`crypto` provides digest algorithms, HMAC, encodings, and cryptographically secure randomness.

```1y
import crypto;

let h = crypto.sha256("hello");
let mac = crypto.hmac_sha256("secret", "payload");
let b64 = crypto.base64_encode(raw_bytes);
let hex = crypto.hex_encode(raw_bytes);

let token = crypto.random_bytes(32);
let dice = crypto.secure_int(1, 6);
let rnd = crypto.secure_float();
```

It supports `sha256`/`sha512`/`sha1`/`md5` digests, `hmac_sha256`/`hmac_sha512` message authentication codes, `base64`/`hex` encode/decode, and a CSPRNG. **Do not use `md5` or `sha1` for security purposes** — both have demonstrated collision attacks and are suitable only for non-security uses like checksums.

## tls — TLS Client

`tls` builds on rustls to provide secure TLS client connections; its API resembles `socket`'s but brings encryption and certificate validation built in.

```1y
import tls;

let conn = tls.connect("example.com", 443);
tls.write(conn, "GET / HTTP/1.1");
let line = tls.read_line(conn);
print(tls.peer_addr(conn));
tls.close(conn);
```

`connect` performs a full TLS handshake including certificate-chain validation and errors if the handshake fails. Once established, `read`/`write` operate over the encrypted channel — you never deal with encryption details. `tls` is currently client-oriented, suited to making HTTPS requests and similar.

## ffi — Dynamic Library Loading

`ffi` loads dynamic libraries and calls C functions within them — the bridge that connects 1y to the native ecosystem. See [Foreign Function Interface](./ffi) for the full treatment.

```1y
import ffi;

let lib = ffi.load("libc.so.6");
let r = ffi.call(lib, "abs", "int(int)", [-42]);
ffi.unload(lib);
```

Because FFI crosses 1y's safety boundary, use it with care: ensure signatures are accurate and libraries come from trusted sources.

## Importing and Using

Standard library modules are brought in with `import` and called as `module.function`:

```1y
import io;
import json;
import crypto;

io.write("out", json.pretty({ salt: crypto.random_bytes(16) }));
```

Modules use **lazy import**: they are only truly loaded on first use, and the same module is loaded at most once across the whole program. You can also alias a module to shorten calls:

```1y
import crypto as c;
let h = c.sha256("x");
```

The standard library is deliberately lean — it includes only cross-platform, dependency-free basics. More specialized capabilities (database drivers, HTTP frameworks, etc.) are left to the ecosystem as third-party packages, while low-level interaction with the operating system is achieved through `ffi` by calling dynamic libraries directly.

## Global Built-in Functions

In addition to the 10 modules above, 1y ships a set of **global functions** — callable without `import`. Full descriptions in [Reflection & Dynamic Evaluation](./introspection); below is an index by category.

### I/O

| Function | Effect |
|----------|--------|
| `println(v)` / `print(v)` | print to stdout (println adds newline) |

### Collection operations

| Function | Effect |
|----------|--------|
| `count(coll)` | element count (Vec/Map/Set/Str) |
| `first(coll)` / `rest(coll)` | first element / collection without first |
| `cons(x, xs)` / `push(xs, x)` | prepend / append |
| `get(coll, k)` / `has_key(m, k)` | index lookup / Map key existence |
| `assoc(m, k, v)` / `dissoc(m, k)` | Map add / remove key |
| `iter_to_vec(iterable)` | materialize any iterable as a Vec |
| `keys(m)` / `values(m)` / `fields(struct)` | Map/Struct keys / values / field pairs |

### Type predicates & reflection

| Function | Effect |
|----------|--------|
| `is_int` / `is_decimal` / `is_str` / `is_bool` / `is_nil` / `is_vec` / `is_map` / `is_set` / `is_number` / `is_func` / `is_closure` | type tests |
| `type_of(v)` | type name string |
| `instance_of(v, name)` | type-name match (with Str↔String, Func↔Closure normalization) |
| `variant_name(v)` / `variant_args(v)` | Variant constructor name / carried values |
| `ast_of(src)` | parse source string into AST data |
| `eval(src)` | dynamically evaluate a source string |

### Arithmetic & numbers

| Function | Effect |
|----------|--------|
| `pow(a, b)` / `abs(n)` | power / absolute value |
| `min(a, b)` / `max(a, b)` | smaller / larger |
| `floor` / `ceil` / `round` / `sqrt` / `sin` / `cos` / `log` / `exp` | math functions |
| `to_i64(v)` / `to_f64(v)` / `int(v)` / `decimal(v)` | numeric conversions |

### Strings

| Function | Effect |
|----------|--------|
| `len(s)` / `split(s, sep)` / `join(xs, sep)` | length / split / join |
| `replace(s, a, b)` / `trim(s)` / `contains(s, sub)` | replace / trim / contains |
| `substring(s, start, end)` | substring |
| `starts_with` / `ends_with` / `index_of` / `char_at` / `codepoint_at` / `from_codepoint` | other string ops |
| `byte_at(s, i)` / `byte_len(s)` | byte-level access |
| `to_lower(s)` / `to_upper(s)` | case conversion |
| `is_digit(c)` / `is_alpha(c)` / `is_space(c)` | character classification |

### Higher-order functions

| Function | Effect |
|----------|--------|
| `map(f, xs)` / `filter(pred, xs)` / `fold(f, init, xs)` / `reduce(f, xs)` / `find(pred, xs)` / `each(f, xs)` | list transformations |

### Conversions

| Function | Effect |
|----------|--------|
| `str(v)` / `to_str(v)` | to string |

### Task combinators

| Function | Effect |
|----------|--------|
| `task_all(tasks)` / `task_any(tasks)` / `task_ready(t)` | async composition (see [Tasks](#tasks)) |

### Actor introspection

| Function | Effect |
|----------|--------|
| `pid_of(actor)` | return the actor's Pid (u64) |

### Time

| Function | Effect |
|----------|--------|
| `now_ms()` | wall-clock milliseconds since UNIX epoch (Int) |
| `now_ns()` | wall-clock nanoseconds since UNIX epoch (Int, higher resolution) |

Used for benchmarking and timing:

```1y
let t0 = now_ms();
// ... work ...
println("elapsed: " + to_str(now_ms() - t0) + " ms")
```
