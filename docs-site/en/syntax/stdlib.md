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
| `process` | Process control | `exit`, `exec`, `exec_status`, `pid`, `cwd`, `set_cwd`, `sleep_ms` |
| `random` | PRNG (xorshift64) | `int`, `range`, `float`, `bool`, `pick`, `shuffle`, `seed` |
| `serial` | Serial port | `open`, `list`, `read`, `write`, `close` |
| `socket` | TCP networking | `listen`, `accept`, `connect`, `read`, `read_line`, `write`, `close`, `set_nonblocking`, `peer_addr` |
| `crypto` | Hashing / CSPRNG | `sha256`, `sha512`, `sha1`, `md5`, `hmac_sha256`, `hmac_sha512`, `base64_encode/decode`, `hex_encode/decode`, `random_bytes`, `secure_int`, `secure_float` |
| `tls` | TLS client (rustls) | `connect`, `read`, `read_line`, `write`, `close`, `peer_addr` |
| `ffi` | Dynamic library loading | `load`, `call`, `unload`, `is_loaded` |

## env — Environment Variables

`env` reads and modifies the process's environment variables and accesses command-line arguments.

```1y
import env;

let home = env.get("HOME");
env.set("MODE", "debug");
env.unset("TEMP");

let argv = env.args();        # argument vector from startup
let all = env.vars();         # all current environment variables
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
    let line = io.read_line();   # read a line from standard input
    io.write("-", line);
}
```

`read_line` reads a line from standard input; `read_to_string` reads an entire file into a string at once. `write` overwrites and `append` appends. When a file is missing, `exists` returns `false` whereas `read_to_string` errors — probe with `exists` first, or catch the error with `match`/`try`.

## json — JSON Codec

`json` converts bidirectionally between 1y values and JSON text.

```1y
import json;

let obj = { name: "1y", version: 1 };
let compact = json.stringify(obj);
let pretty = json.pretty(obj);   # indented, for human reading

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

process.sleep_ms(500);     # sleep for 500 milliseconds
process.exit(0);
```

`exec` runs a subcommand and blocks until it finishes; `exec_status` returns the exit code; `exit` terminates the current process immediately. `sleep_ms` pauses the current execution flow for the given milliseconds — under the event-loop model it yields the scheduler, giving other Actors a chance to run.

## random — Pseudo-Random Numbers

`random` is based on the xorshift64 algorithm and provides fast but **not cryptographically secure** pseudo-random numbers.

```1y
import random;

random.seed(42);
let n = random.int();              # arbitrary integer
let r = random.range(1, 100);      # [1, 100)
let f = random.float();            # [0.0, 1.0)
let b = random.bool();
let choice = random.pick([1, 2, 3]);
let shuffled = random.shuffle([1, 2, 3, 4]);
```

For cryptographically secure randomness, use the `crypto` module's `random_bytes` / `secure_int` / `secure_float` instead. `seed` fixes the sequence for reproducible tests; if you never call `seed`, a default seed is used.

## serial — Serial Port

`serial` interacts with serial devices, common in embedded and industrial settings.

```1y
import serial;

let ports = serial.list();                 # enumerate available ports
let dev = serial.open("COM3", 115200);     # or /dev/ttyUSB0
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

# client
let c = socket.connect("example.com", 80);
socket.write(c, "GET / HTTP/1.0");
socket.close(c);
```

`listen` creates a listening socket and `accept` blocks waiting for and returns a new connection. `set_nonblocking` makes subsequent `read`/`read_line` return immediately when no data is available rather than hanging, which pairs well with an event loop for concurrent serving. `peer_addr` returns the remote address for logging and authorization.

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
