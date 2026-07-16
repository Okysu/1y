# `1y` Standard Library Reference

This document lists every function in the `1y` standard library as of
Phase C (colorless async + BEAM actor model).

## Built-in Functions (global)

These are available without any `import`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `println` | `(v?) -> Nil` | Print value + newline |
| `print` | `(v?) -> Nil` | Print value, no newline |
| `pow` | `(Int, Int) -> Int` | Integer power |
| `abs` | `(Int or Decimal) -> same` | Absolute value |
| `count` | `(Vec/Map/Set/Str) -> Int` | Collection size |
| `first` | `(Vec) -> Value` | First element |
| `rest` | `(Vec) -> Vec` | All but first |
| `cons` | `(Value, Vec) -> Vec` | Prepend |
| `push` | `(Vec, Value) -> Vec` | Append |
| `assoc` | `(Map, Key, Val) -> Map` | Add/update key |
| `dissoc` | `(Map, Key) -> Map` | Remove key |
| `get` | `(Map, Key) -> Value` | Lookup |
| `is_int` / `is_decimal` / `is_str` / `is_bool` / `is_nil` / `is_vec` / `is_map` / `is_set` / `is_func` / `is_number` | `(Value) -> Bool` | Type predicates |
| `type_of` | `(Value) -> Str` | Type name string |
| `to_i64` | `(Value) -> Int` | Convert to Int |
| `to_f64` | `(Value) -> Decimal` | Convert to Decimal |
| `int` | `(Value) -> Int` | Convert to Int |
| `decimal` | `(Value) -> Decimal` | Convert to Decimal |
| `str` | `(Value) -> Str` | Convert to Str (Display) |
| `map` | `(Vec, Func) -> Vec` | Apply function to each |
| `filter` | `(Vec, Func) -> Vec` | Keep matching |
| `fold` | `(Vec, Value, Func) -> Value` | Left fold with init |
| `reduce` | `(Vec, Func) -> Value` | Left fold, first as init |
| `find` | `(Vec, Func) -> Value or Nil` | First matching |
| `each` | `(Vec, Func) -> Nil` | Iterate, side effects |
| `len` | `(Str) -> Int` | String length |
| `split` | `(Str, Str) -> Vec` | Split by separator |
| `join` | `(Vec, Str) -> Str` | Join with separator |
| `replace` | `(Str, Str, Str) -> Str` | Replace all |
| `trim` | `(Str) -> Str` | Strip whitespace |
| `contains` | `(Str, Str) -> Bool` | Substring test |
| `substring` | `(Str, Int, Int) -> Str` | Slice [start, end) |
| `min` / `max` | `(Value...) -> Value` | Min/max of numbers |
| `floor` / `ceil` / `round` | `(Decimal) -> Int` | Rounding |
| `sqrt` | `(Value) -> Decimal` | Square root |
| `sin` / `cos` | `(Value) -> Decimal` | Trig (radians) |
| `log` | `(Value) -> Decimal` | Natural log |
| `exp` | `(Value) -> Decimal` | e^x |
| `pid_of` | `(Actor) -> Int` | Actor's global Pid (Phase C3) |
| `task_ready` | `(Value) -> Task` | Immediately-ready Task |
| `task_all` | `(Vec<Task>) -> Task` | Resolves when all inputs resolve |
| `task_any` | `(Vec<Task>) -> Task` | Resolves when any input resolves |

## `env`

| Function | Signature | Description |
|----------|-----------|-------------|
| `get` | `(Str) -> Str or Nil` | Get env var |
| `set` | `(Str, Str)` | Set env var |
| `unset` | `(Str)` | Remove env var |
| `args` | `() -> Vec` | Command-line args |
| `vars` | `() -> Map` | All env vars |

## `io`

| Function | Signature | Description |
|----------|-----------|-------------|
| `read_line` | `() -> Str or Nil` | Read stdin line |
| `read_to_string` | `(Str) -> Str` | Read file to string |
| `write` | `(Str, Str)` | Write string to file |
| `append` | `(Str, Str)` | Append string to file |
| `exists` | `(Str) -> Bool` | File exists |

## `json`

| Function | Signature | Description |
|----------|-----------|-------------|
| `parse` | `(Str) -> Value` | Parse JSON to 1y value |
| `stringify` | `(Value) -> Str` | Serialize to compact JSON |
| `pretty` | `(Value, Int) -> Str` | Serialize to indented JSON |

- Integers → `Int`, decimals → `Decimal` (preserves precision)
- Objects → `Map`, arrays → `Vec`, strings → `Str`

## `process`

| Function | Signature | Description |
|----------|-----------|-------------|
| `exit` | `(Int) -> !` | Exit process |
| `exec` | `(Str, Vec) -> Str` | Run command, return stdout |
| `exec_status` | `(Str, Vec) -> Int` | Run command, return exit code |
| `pid` | `() -> Int` | Current process ID |
| `cwd` | `() -> Str` | Current working directory |
| `set_cwd` | `(Str)` | Change working directory |
| `sleep_ms` | `(Int)` | Sleep milliseconds (blocking, main thread only) |
| `sleep_async` | `(Int) -> Task` | Sleep milliseconds as a Task (colorless async; `await` it) |

## `random`

PRNG is xorshift64, thread-local, **NOT cryptographically secure**. Use `crypto`
module for security-sensitive randomness.

| Function | Signature | Description |
|----------|-----------|-------------|
| `int` | `(Int) -> Int` | Random in [0, max) |
| `range` | `(Int, Int) -> Int` | Random in [min, max) |
| `float` | `() -> Decimal` | Random in [0, 1) |
| `bool` | `() -> Bool` | Random boolean |
| `pick` | `(Vec) -> Value` | Random element |
| `shuffle` | `(Vec) -> Vec` | Shuffle copy |
| `seed` | `(Int)` | Seed the PRNG |

## `serial`

Based on the `serialport` crate.

| Function | Signature | Description |
|----------|-----------|-------------|
| `open` | `(Str, Int) -> Opaque` | Open port with baud rate |
| `open_full` | `(Str, Map) -> Opaque` | Open with full config |
| `read` | `(Opaque, Int) -> Str or Nil` | Read up to N bytes |
| `read_line` | `(Opaque) -> Str or Nil` | Read until newline |
| `write` | `(Opaque, Str)` | Write string |
| `close` | `(Opaque)` | Close port |
| `list` | `() -> Vec` | List available ports |

`open_full` config map keys: `baud`, `data_bits`, `parity`, `stop_bits`,
`flow_control`, `timeout_ms`.

## `socket`

TCP networking. Supports both blocking and non-blocking modes. Non-blocking
mode + `read_async` enables colorless async I/O (suspend on `WouldBlock`,
resume when data arrives — other coroutines run in the meantime).

| Function | Signature | Description |
|----------|-----------|-------------|
| `listen` | `(Str) -> Opaque` | Bind TCP listener ("addr:port") |
| `accept` | `(Opaque) -> Opaque or Nil` | Accept connection → stream; Nil if none pending (non-blocking) |
| `connect` | `(Str) -> Opaque` | Connect to "addr:port" |
| `read` | `(Opaque, Int) -> Str or Nil` | Read up to N bytes (blocking) |
| `read_async` | `(Opaque, Int) -> Task` | Read up to N bytes as a Task (colorless async; `await` it) |
| `read_line` | `(Opaque) -> Str or Nil` | Read until newline |
| `write` | `(Opaque, Str)` | Write string |
| `close` | `(Opaque)` | Close socket |
| `set_nonblocking` | `(Opaque, Bool)` | Set non-blocking mode |
| `peer_addr` | `(Opaque) -> Str` | Remote address |

## `crypto`

Cryptographic primitives using RustCrypto crates.

| Function | Signature | Description |
|----------|-----------|-------------|
| `sha256` | `(Str) -> Str` | SHA-256, hex output |
| `sha512` | `(Str) -> Str` | SHA-512, hex output |
| `sha1` | `(Str) -> Str` | SHA-1, hex output |
| `md5` | `(Str) -> Str` | MD5, hex output |
| `hmac_sha256` | `(Str, Str) -> Str` | HMAC-SHA-256, hex |
| `hmac_sha512` | `(Str, Str) -> Str` | HMAC-SHA-512, hex |
| `base64_encode` | `(Str) -> Str` | Base64 encode |
| `base64_decode` | `(Str) -> Str` | Base64 decode |
| `hex_encode` | `(Str) -> Str` | Hex encode |
| `hex_decode` | `(Str) -> Str` | Hex decode |
| `random_bytes` | `(Int) -> Vec<Int>` | CSPRNG bytes (OsRng) |
| `secure_int` | `(Int) -> Int` | CSPRNG int in [0, max) |
| `secure_float` | `() -> Decimal` | CSPRNG float in [0, 1) |

## `tls`

TLS client connections via `rustls` 0.23 with `ring` provider and Mozilla root
CAs.

| Function | Signature | Description |
|----------|-----------|-------------|
| `connect` | `(Str, Int) -> Opaque` | TLS connect to host:port |
| `read` | `(Opaque, Int) -> Str or Nil` | Read up to N bytes |
| `read_line` | `(Opaque) -> Str or Nil` | Read until newline |
| `write` | `(Opaque, Str)` | Write string |
| `close` | `(Opaque)` | Close TLS stream |
| `peer_addr` | `(Opaque) -> Str` | Remote address |

The handshake is eager — `connect` performs certificate verification before
returning.

## `ffi`

Dynamic library loading via `libloading`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `load` | `(Str) -> Opaque` | Open shared library |
| `call` | `(Opaque, Str, Str, Vec) -> Value` | Call foreign function |
| `unload` | `(Opaque)` | Close library |
| `is_loaded` | `(Str) -> Bool` | Check file exists at path |

`call` arguments: `(lib, symbol_name, signature, args_vec)`.

Signature format: `"ret(arg1, arg2, ...)"` where types are:
- `void` — no return (Nil)
- `int` — i64
- `uint` — u64
- `float` — f64
- `str` — `*const c_char` (NUL-terminated)

Up to 6 arguments supported. **FFI is unsafe**: only load trusted libraries.

## `lib.http` (self-hosted library)

Pure-1y HTTP/1.1 server library (`lib/http.1y`). Built on the `socket` and
`process` modules with Actor-based concurrency + colorless async. Each
connection runs in a spawned `Connection` actor; the handler may `await` any
`Task` without blocking other connections.

| Function | Signature | Description |
|----------|-----------|-------------|
| `serve` | `(Str, Func) -> !` | Start server on "addr:port"; handler receives a request Map |
| `parse_request` | `(Str) -> Map or Nil` | Parse raw HTTP request string |
| `response` | `(Int, Str, Vec) -> Str` | Build raw HTTP response (status, body, headers) |
| `json_response` | `(Int, Str) -> Str` | Shortcut: JSON response |
| `html_response` | `(Int, Str) -> Str` | Shortcut: HTML response |
| `text_response` | `(Int, Str) -> Str` | Shortcut: plain-text response |
| `status_text` | `(Int) -> Str` | Human-readable status text |
| `not_found` | `() -> Str` | Build a 404 response |

The handler Map shape:
- Request: `{ "method", "path", "version", "headers": Map, "body": Str }`
- Response: `{ "status": Int, "body": Str, "headers": Vec<Str> }`

Headers are a `Vec` of `"Key: Value"` strings (1y has no native Map iteration).

## `lib.yin` (self-hosted web framework)

Gin-inspired web framework (`lib/yin.1y`), built on `lib.http`. Demonstrates
that 1y's language features are sufficient for a real web framework with no
native extensions.

| Function | Signature | Description |
|----------|-----------|-------------|
| `new` | `() -> App` | Create a new app (shared cell holding routes/middlewares) |
| `get` / `post` / `put` / `delete` | `(App, Str, Func) -> Nil` | Register a route handler |
| `use` | `(App, Func) -> Nil` | Register middleware `fn(ctx, next)` |
| `group` | `(App, Str) -> App` | Create a route group with a prefix (shares parent's route table) |
| `handle` | `(App, Map) -> Map` | Dispatch a request Map, return response Map |
| `run` | `(App, Str) -> !` | Start the HTTP server on "addr:port" |
| `param` | `(Ctx, Str) -> Str` | Extract a path parameter (`:id`) |
| `header` | `(Ctx, Str) -> Str` | Get a request header |
| `body` | `(Ctx) -> Str` | Get the request body |
| `json` | `(Ctx, Int, Value) -> Nil` | Write JSON response (sets Content-Type) |
| `html` | `(Ctx, Int, Str) -> Nil` | Write HTML response |
| `text` | `(Ctx, Int, Str) -> Nil` | Write plain-text response |
| `set_header` | `(Ctx, Str, Str) -> Nil` | Append a response header |
| `status_code` | `(Ctx, Int) -> Nil` | Override the status code |

See `examples/yin_server.1y` for a complete example.

## `parallel` (built-in module)

User-facing multi-threading. Built on the `WorkerPool` (N worker threads, one
per CPU core). Functions are called by name on worker threads that pre-load
the entry file's definitions.

| Function | Signature | Description |
|----------|-----------|-------------|
| `cores` | `() -> Int` | Number of CPU cores available |
| `call` | `(Str, Vec) -> Value` | Synchronously call named function with args, return result |
| `spawn` | `(Str, Vec) -> Handle` | Asynchronously call named function, return a handle |
| `join` | `(Handle) -> Value` | Wait for a spawned task, return its result |
| `map` | `(Str, Vec<Vec>) -> Vec` | Call named function in parallel for each arg set |

Arguments and return values must be `SendValue`-compatible (Int, Str, Bool,
Nil, Vec, Map, Set, Variant, Struct). Functions, shared cells, actors, tasks,
and opaque resources cannot cross thread boundaries.

```1y
fn double(n) { n * 2 }

let r = parallel.call("double", [21]);          // 42
let h = parallel.spawn("double", [21]);         // Handle
let r2 = parallel.join(h);                       // 42
let rs = parallel.map("double", [[1],[2],[3]]); // [2, 4, 6]
```
