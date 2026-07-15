---
title: Foreign Function Interface
---

# Foreign Function Interface

1y is a managed language, but the real world is full of C libraries — operating-system interfaces, hardware drivers, legacy systems. Through the `ffi` module, 1y provides a **Foreign Function Interface**: at runtime it loads dynamic libraries (`.so` / `.dll` / `.dylib`) and calls their C functions by signature, bridging 1y to the native ecosystem.

This page covers the four core functions of `ffi` — `load`, `call`, `unload`, `is_loaded` — along with the signature format, the ABI types, and safety considerations.

## Loading a Library: load

`ffi.load(path)` loads the dynamic library at the given path and returns a **library handle**. Subsequent `call`s locate functions through this handle.

```1y
import ffi;

let lib = ffi.load("libc.so.6");          # Linux
# let lib = ffi.load("msvcrt.dll");       # Windows
# let lib = ffi.load("libSystem.dylib");  # macOS
```

`load` searches the standard library paths (influenced by `LD_LIBRARY_PATH` / `PATH` / `DYLD_LIBRARY_PATH` and similar) and also accepts absolute or relative paths. If loading fails, `load` errors with the reason. Loading the same library twice yields independent handles; typically you load once at startup and hold the handle for the program's lifetime.

## Calling a Function: call

`ffi.call(lib, name, signature, args)` calls the C function named `name` in the library. `signature` describes the return and parameter types, and `args` is the list of argument values.

```1y
let r = ffi.call(lib, "abs", "int(int)", [-42]);
print(r);    # 42
```

How `call` works:

1. Look up the function symbol by `name` in the library; error if not found.
2. Parse the return and parameter types from `signature` and convert between 1y values and the C ABI accordingly.
3. Invoke the function with the C calling convention and convert the return value back to a 1y value per the signature.

## Signature Format

A signature is a string of the form `"ret(arg1, arg2, ...)"`: the part before the parentheses is the return type, and inside the parentheses is a comma-separated list of parameter types. A no-argument function is written `"ret()"`.

```1y
"int(int)"          # takes one int, returns int
"void(int)"         # takes one int, returns nothing
"str(str)"          # takes a C string, returns a C string
"float(float)"      # takes an f64, returns an f64
"int(str, int)"     # takes (string, int), returns int
```

A return type of `void` means the function returns nothing, in which case `call` returns the unit value. Whitespace in the signature is ignored, so `"int( int , str )"` is equivalent to `"int(int,str)"`.

## ABI Types

The types in a signature correspond to the C ABI; 1y supports five:

| Signature type | C equivalent | 1y equivalent | Notes |
|----------------|--------------|---------------|-------|
| `void` | `void` | unit | Only valid as a return type |
| `int` | `int64_t` | integer | Signed 64-bit |
| `uint` | `uint64_t` | integer | Unsigned 64-bit |
| `float` | `double` | float | 64-bit floating point |
| `str` | `char*` | string | NUL-terminated C string |

Note that `int` and `uint` both map to 1y's arbitrary-precision integers, but are truncated to 64 bits when crossing the FFI boundary; values outside that range produce unexpected wrapping, so you must ensure they are in range yourself. When passed in, a `str` is converted from a 1y string to a UTF-8 C string, and converted back on return — if the C function returns something that is not valid UTF-8, the conversion may lose bytes.

## Argument Count Limit

`ffi.call` supports **at most 6 arguments**. This cap exists so that 1y's FFI implementation keeps register/stack argument passing simple and consistent across platforms. The vast majority of C functions take far fewer arguments; if you genuinely need more, wrap the call in a C function with fewer parameters, or pack the arguments into a struct pointer and pass the pointer as `int(int)`.

## Unloading and Probing: unload / is_loaded

`ffi.unload(lib)` unloads the library handle and releases its underlying resources. After unloading the handle is unusable; calling `call` on it errors.

`ffi.is_loaded(lib)` probes whether the library for a handle is still loaded, returning a boolean — handy for safe checks when you're unsure of the lifetime.

```1y
if ffi.is_loaded(lib) {
    ffi.call(lib, "cleanup", "void()", []);
    ffi.unload(lib);
}
```

One important caveat: if a `call` returns a `str` (or, in the future, a pointer) that points at static data inside the library, accessing that data after `unload` is undefined behavior. The safest approach is to **finish using all results before unloading the library**.

## Safety

FFI is **inherently unsafe**. The moment you cross the FFI boundary, 1y's memory safety, type safety, and isolation guarantees all cease to apply:

- **Signatures must be correct**: if the signature you declare does not match the C function's actual signature (wrong types, wrong argument count), the call may trigger undefined behavior, out-of-bounds reads, or even a crash. 1y cannot check signatures at compile time — it trusts the string you wrote.
- **Load only trusted libraries**: code in a dynamic library runs with full privileges — it can read and write process memory and invoke arbitrary syscalls. Never `load` a library from an untrusted source; doing so is equivalent to executing arbitrary code.
- **Lifetime and threading**: a library handle is unusable after unloading; if a C function holds pointers across calls, 1y's GC may reclaim the corresponding memory without your noticing, so ownership must be managed very carefully.
- **Platform differences**: library paths, symbol visibility, and calling conventions can vary by platform, so cross-platform code should branch per platform (e.g., `libc.so.6` on Linux, `msvcrt.dll` on Windows).

For these reasons, FFI should be a **last resort**: first check whether the standard library already offers the capability (`crypto`, `socket`, `tls`, etc.); only when there is no alternative should you reach for FFI, and even then encapsulate the unsafe calls inside one small, thick module that exposes a safe 1y interface, confining the danger to the smallest possible surface.

## A Complete Example

```1y
import ffi;
import io;

# Load the C standard library cross-platform and call abs / strlen
let lib = ffi.load("libc.so.6");

let absval = ffi.call(lib, "abs", "int(int)", [-42]);
println("abs(-42) = " + str(absval));

let len = ffi.call(lib, "strlen", "uint(str)", ["hello"]);
println("strlen(hello) = " + str(len));

ffi.unload(lib);
```

FFI ensures 1y is not stranded on an "ecosystem island": anything callable from C is callable from 1y. But with great power comes great responsibility — make sure signatures are accurate, libraries are trusted, and calls are properly encapsulated. When the standard library already covers a capability, prefer it; leave FFI for the cases where there is no other way.
