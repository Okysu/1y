# `1y` Standard Library Reference

This document lists every function in the `1y` standard library as of Phase 4.6.

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
| `sleep_ms` | `(Int)` | Sleep milliseconds |

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

Blocking TCP networking.

| Function | Signature | Description |
|----------|-----------|-------------|
| `listen` | `(Str) -> Opaque` | Bind TCP listener ("addr:port") |
| `accept` | `(Opaque) -> Opaque` | Accept connection → stream |
| `connect` | `(Str) -> Opaque` | Connect to "addr:port" |
| `read` | `(Opaque, Int) -> Str or Nil` | Read up to N bytes |
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
