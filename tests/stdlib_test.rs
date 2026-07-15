//! Phase 4 standard library tests.
//!
//! Covers env, io, random, json, and process modules.

use onely::Interpreter;
use onely::Value;

fn eval(src: &str) -> Value {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(v) => v,
        Err(e) => panic!("eval failed: {}", e),
    }
}

fn eval_str(src: &str) -> String {
    format!("{}", eval(src))
}

fn eval_err(src: &str) -> String {
    let mut interp = Interpreter::new();
    match interp.eval_source(src) {
        Ok(_) => panic!("expected error, got success"),
        Err(e) => format!("{}", e),
    }
}

// ---------------------------------------------------------------------------
// env module
// ---------------------------------------------------------------------------

#[test]
fn test_env_set_get() {
    let v = eval(r#"
        import env;
        env.set("ONELY_TEST_VAR", "hello");
        env.get("ONELY_TEST_VAR")
    "#);
    assert_eq!(v, Value::str("hello"));
}

#[test]
fn test_env_get_unset_returns_nil() {
    let v = eval(r#"import env; env.get("ONELY_NO_SUCH_VAR_XYZ")"#);
    assert_eq!(v, Value::Nil);
}

#[test]
fn test_env_unset() {
    let v = eval(r#"
        import env;
        env.set("ONELY_TEST_UNSET", "value");
        env.unset("ONELY_TEST_UNSET");
        env.get("ONELY_TEST_UNSET")
    "#);
    assert_eq!(v, Value::Nil);
}

#[test]
fn test_env_args() {
    let v = eval(r#"import env; env.args()"#);
    assert!(matches!(v, Value::Vec(_)));
}

#[test]
fn test_env_vars() {
    let v = eval(r#"import env; env.vars()"#);
    assert!(matches!(v, Value::Vec(_)));
}

// ---------------------------------------------------------------------------
// io module
// ---------------------------------------------------------------------------

#[test]
fn test_io_write_read() {
    let path = std::env::temp_dir().join("onely_io_test.txt");
    let path_str = path.to_str().unwrap().replace('\\', "/");
    let src = format!(r#"
        import io;
        io.write("{}", "hello world");
        io.read_to_string("{}")
    "#, path_str, path_str);
    let v = eval(&src);
    assert_eq!(v, Value::str("hello world"));
}

#[test]
fn test_io_append() {
    let path = std::env::temp_dir().join("onely_io_append.txt");
    let path_str = path.to_str().unwrap().replace('\\', "/");
    let src = format!(r#"
        import io;
        io.write("{}", "first");
        io.append("{}", " second");
        io.read_to_string("{}")
    "#, path_str, path_str, path_str);
    let v = eval(&src);
    assert_eq!(v, Value::str("first second"));
}

#[test]
fn test_io_exists() {
    let v = eval(r#"import io; io.exists(".")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_io_exists_false() {
    let v = eval(r#"import io; io.exists("no_such_file_xyz.1y")"#);
    assert_eq!(v, Value::Bool(false));
}

// ---------------------------------------------------------------------------
// random module
// ---------------------------------------------------------------------------

#[test]
fn test_random_seed_deterministic() {
    // Same seed should produce the same sequence.
    let src1 = r#"
        import random;
        random.seed(42);
        str(random.int(1000)) + "," + str(random.int(1000))
    "#;
    let src2 = r#"
        import random;
        random.seed(42);
        str(random.int(1000)) + "," + str(random.int(1000))
    "#;
    assert_eq!(eval_str(src1), eval_str(src2));
}

#[test]
fn test_random_int_range() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.int(10)
    "#);
    match v {
        Value::Int(n) => {
            assert!(n >= num_bigint::BigInt::from(0));
            assert!(n < num_bigint::BigInt::from(10));
        }
        _ => panic!("expected Int, got {}", v),
    }
}

#[test]
fn test_random_range() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.range(100, 200)
    "#);
    match v {
        Value::Int(n) => {
            assert!(n >= num_bigint::BigInt::from(100));
            assert!(n < num_bigint::BigInt::from(200));
        }
        _ => panic!("expected Int, got {}", v),
    }
}

#[test]
fn test_random_float() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.float()
    "#);
    assert!(matches!(v, Value::Decimal(_)));
}

#[test]
fn test_random_bool() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.bool()
    "#);
    assert!(matches!(v, Value::Bool(_)));
}

#[test]
fn test_random_pick() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.pick([10, 20, 30, 40])
    "#);
    match v {
        Value::Int(n) => {
            let valid = [10, 20, 30, 40].iter().any(|x| num_bigint::BigInt::from(*x) == n);
            assert!(valid, "picked value not in array: {}", n);
        }
        _ => panic!("expected Int, got {}", v),
    }
}

#[test]
fn test_random_shuffle() {
    let v = eval(r#"
        import random;
        random.seed(42);
        random.shuffle([1, 2, 3, 4, 5])
    "#);
    assert!(matches!(v, Value::Vec(_)));
    let len = if let Value::Vec(arr) = v { arr.len() } else { 0 };
    assert_eq!(len, 5);
}

#[test]
fn test_random_int_error_on_zero() {
    let err = eval_err(r#"import random; random.int(0)"#);
    assert!(err.contains("must be positive"), "got: {}", err);
}

#[test]
fn test_random_range_error() {
    let err = eval_err(r#"import random; random.range(100, 50)"#);
    assert!(err.contains("must be greater"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// json module
// ---------------------------------------------------------------------------

/// Helper: eval and extract the raw string content (without Display quotes).
fn eval_string(src: &str) -> String {
    match eval(src) {
        Value::Str(s) => (*s).clone(),
        v => panic!("expected Str, got {}", v),
    }
}

#[test]
fn test_json_parse_simple() {
    let v = eval(r#"import json; json.parse("42")"#);
    assert_eq!(v, Value::int(42));
}

#[test]
fn test_json_parse_string() {
    let v = eval_string(r#"import json; json.parse("\"hello\"")"#);
    assert_eq!(v, "hello");
}

#[test]
fn test_json_parse_bool() {
    let v = eval(r#"import json; json.parse("true")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_json_parse_null() {
    let v = eval(r#"import json; json.parse("null")"#);
    assert_eq!(v, Value::Nil);
}

#[test]
fn test_json_parse_array() {
    let v = eval(r#"import json; json.parse("[1, 2, 3]")"#);
    assert_eq!(v, Value::vec(vec![Value::int(1), Value::int(2), Value::int(3)]));
}

#[test]
fn test_json_parse_object() {
    // Use \{ and \} to escape interpolation in 1y strings.
    let v = eval(r#"import json; json.parse("\{\"a\": 1, \"b\": 2\}")"#);
    match v {
        Value::Map(m) => {
            assert_eq!(m.len(), 2);
            // Keys are Value::Str, values are Value::Int
            let a_key = Value::str("a");
            let b_key = Value::str("b");
            assert_eq!(m.get(&a_key), Some(&Value::int(1)));
            assert_eq!(m.get(&b_key), Some(&Value::int(2)));
        }
        _ => panic!("expected Map, got {}", v),
    }
}

#[test]
fn test_json_stringify() {
    let v = eval_string(r#"import json; json.stringify([1, 2, 3])"#);
    assert_eq!(v, "[1,2,3]");
}

#[test]
fn test_json_stringify_object() {
    let v = eval_string(r#"import json; json.stringify(\{ "a": 1 \})"#);
    assert_eq!(v, "{\"a\":1}");
}

#[test]
fn test_json_stringify_null_bool() {
    let v = eval_string(r#"import json; json.stringify(nil)"#);
    assert_eq!(v, "null");
}

#[test]
fn test_json_roundtrip() {
    // json.parse returns a Value::Map, not a Str — check structure directly.
    let v = eval(r#"
        import json;
        let data = \{ "name": "Alice", "age": 30 \};
        json.parse(json.stringify(data))
    "#);
    match v {
        Value::Map(m) => {
            assert_eq!(m.len(), 2, "roundtrip should preserve both keys");
            assert_eq!(
                m.get(&Value::str("name")),
                Some(&Value::str("Alice")),
                "name field should roundtrip"
            );
            assert_eq!(
                m.get(&Value::str("age")),
                Some(&Value::int(30)),
                "age field should roundtrip"
            );
        }
        _ => panic!("expected Map after roundtrip, got {}", v),
    }
}

#[test]
fn test_json_bigint_preservation() {
    let v = eval_string(r#"import json; str(json.parse("99999999999999999999999"))"#);
    assert_eq!(v, "99999999999999999999999");
}

#[test]
fn test_json_pretty() {
    let v = eval_string(r#"import json; json.pretty([1, 2], 2)"#);
    assert!(v.contains("\n"), "pretty output should have newlines, got: {}", v);
}

#[test]
fn test_json_parse_error() {
    // Escape { and } to avoid 1y string interpolation.
    let err = eval_err(r#"import json; json.parse("\{invalid\}")"#);
    assert!(err.contains("json.parse"), "got: {}", err);
}

#[test]
fn test_json_stringify_escapes() {
    // 1y string "hello\nworld" contains a real newline; JSON should escape it.
    let v = eval_string(r#"import json; json.stringify("hello\nworld")"#);
    assert_eq!(v, "\"hello\\nworld\"");
}

#[test]
fn test_json_nested() {
    let v = eval_string(r#"import json; json.stringify(\{ "a": [1, 2], "b": \{ "c": true \} \})"#);
    assert!(v.contains("\"a\":[1,2]"), "got: {}", v);
    assert!(v.contains("\"b\":{\"c\":true}"), "got: {}", v);
}

// ---------------------------------------------------------------------------
// process module
// ---------------------------------------------------------------------------

#[test]
fn test_process_pid() {
    let v = eval(r#"import process; process.pid()"#);
    assert!(matches!(v, Value::Int(_)));
}

#[test]
fn test_process_cwd() {
    let v = eval(r#"import process; process.cwd()"#);
    assert!(matches!(v, Value::Str(_)));
}

#[test]
fn test_process_set_cwd() {
    let v = eval(r#"
        import process;
        let orig = process.cwd();
        process.set_cwd(".");
        process.cwd() == orig
    "#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_process_sleep_ms() {
    let v = eval(r#"import process; process.sleep_ms(1)"#);
    assert_eq!(v, Value::Nil);
}

#[test]
fn test_process_exec_status() {
    // Use a cross-platform command: `cmd /c echo` on Windows, `echo` on Unix.
    #[cfg(windows)]
    let src = r#"import process; process.exec_status("cmd", ["/c", "exit", "0"])"#;
    #[cfg(not(windows))]
    let src = r#"import process; process.exec_status("true", [])"#;
    let v = eval(src);
    assert_eq!(v, Value::int(0));
}

// ---------------------------------------------------------------------------
// socket module (basic — no network needed)
// ---------------------------------------------------------------------------

#[test]
fn test_socket_module_import() {
    let v = eval(r#"import socket; socket"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_socket_connect_error() {
    // Connecting to an unlikely port should fail.
    let err = eval_err(r#"import socket; socket.connect("127.0.0.1:1")"#);
    assert!(err.contains("socket.connect"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// serial module (basic — no hardware needed)
// ---------------------------------------------------------------------------

#[test]
fn test_serial_module_import() {
    let v = eval(r#"import serial; serial"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_serial_list() {
    // list() should return a Vec (possibly empty on machines without serial ports).
    let v = eval(r#"import serial; serial.list()"#);
    assert!(matches!(v, Value::Vec(_)));
}

#[test]
fn test_serial_open_error() {
    let err = eval_err(r#"import serial; serial.open("COM_NONEXISTENT", 9600)"#);
    assert!(err.contains("serial.open"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// crypto module (Phase 4.5)
// ---------------------------------------------------------------------------

#[test]
fn test_crypto_module_import() {
    let v = eval(r#"import crypto; crypto"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_crypto_sha256_abc() {
    // SHA-256("abc") is a well-known test vector.
    let v = eval_string(r#"import crypto; crypto.sha256("abc")"#);
    assert_eq!(v, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn test_crypto_sha256_empty() {
    let v = eval_string(r#"import crypto; crypto.sha256("")"#);
    assert_eq!(v, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn test_crypto_sha512_abc() {
    let v = eval_string(r#"import crypto; crypto.sha512("abc")"#);
    assert_eq!(v, "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
}

#[test]
fn test_crypto_sha1_abc() {
    let v = eval_string(r#"import crypto; crypto.sha1("abc")"#);
    assert_eq!(v, "a9993e364706816aba3e25717850c26c9cd0d89d");
}

#[test]
fn test_crypto_md5_abc() {
    let v = eval_string(r#"import crypto; crypto.md5("abc")"#);
    assert_eq!(v, "900150983cd24fb0d6963f7d28e17f72");
}

#[test]
fn test_crypto_hmac_sha256() {
    // HMAC-SHA256("key", "The quick brown fox jumps over the lazy dog")
    let v = eval_string(r#"import crypto; crypto.hmac_sha256("key", "The quick brown fox jumps over the lazy dog")"#);
    assert_eq!(v, "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
}

#[test]
fn test_crypto_hmac_sha512() {
    // HMAC-SHA512("key", "The quick brown fox jumps over the lazy dog") — just
    // check length (128 hex chars = 64 bytes) and that it's hex.
    let v = eval_string(r#"import crypto; crypto.hmac_sha512("key", "The quick brown fox jumps over the lazy dog")"#);
    assert_eq!(v.len(), 128, "HMAC-SHA512 should be 64 bytes / 128 hex chars");
    assert!(v.chars().all(|c| c.is_ascii_hexdigit()), "should be hex");
}

#[test]
fn test_crypto_base64_roundtrip() {
    let v = eval_string(r#"import crypto; crypto.base64_decode(crypto.base64_encode("hello world"))"#);
    assert_eq!(v, "hello world");
}

#[test]
fn test_crypto_base64_known() {
    // base64("hello") = "aGVsbG8="
    let v = eval_string(r#"import crypto; crypto.base64_encode("hello")"#);
    assert_eq!(v, "aGVsbG8=");
}

#[test]
fn test_crypto_hex_roundtrip() {
    let v = eval_string(r#"import crypto; crypto.hex_decode(crypto.hex_encode("1y"))"#);
    assert_eq!(v, "1y");
}

#[test]
fn test_crypto_hex_encode_known() {
    // hex("A") = "41"
    let v = eval_string(r#"import crypto; crypto.hex_encode("A")"#);
    assert_eq!(v, "41");
}

#[test]
fn test_crypto_hex_decode_error() {
    let err = eval_err(r#"import crypto; crypto.hex_decode("xyz")"#);
    assert!(err.contains("hex_decode"), "got: {}", err);
}

#[test]
fn test_crypto_random_bytes() {
    let v = eval(r#"import crypto; crypto.random_bytes(16)"#);
    match v {
        Value::Vec(vec) => {
            assert_eq!(vec.len(), 16, "should return 16 bytes");
            for b in vec.iter() {
                match b {
                    Value::Int(n) => {
                        let v = n.to_string().parse::<i64>().unwrap_or(-1);
                        assert!(v >= 0 && v < 256, "byte out of range: {}", v);
                    }
                    other => panic!("expected Int byte, got {:?}", other),
                }
            }
        }
        other => panic!("expected Vec, got {:?}", other),
    }
}

#[test]
fn test_crypto_secure_int() {
    let v = eval(r#"import crypto; crypto.secure_int(100)"#);
    match v {
        Value::Int(n) => {
            let i = n.to_string().parse::<i64>().unwrap_or(-1);
            assert!(i >= 0 && i < 100, "secure_int out of range: {}", i);
        }
        other => panic!("expected Int, got {:?}", other),
    }
}

#[test]
fn test_crypto_secure_int_zero_error() {
    let err = eval_err(r#"import crypto; crypto.secure_int(0)"#);
    assert!(err.contains("secure_int"), "got: {}", err);
}

#[test]
fn test_crypto_secure_float() {
    let v = eval(r#"import crypto; crypto.secure_float()"#);
    assert!(matches!(v, Value::Decimal(_)), "expected Decimal, got {:?}", v);
}

#[test]
fn test_crypto_sha256_type_error() {
    let err = eval_err(r#"import crypto; crypto.sha256(42)"#);
    assert!(err.contains("sha256"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// tls module (Phase 4.5 — basic import, no live network)
// ---------------------------------------------------------------------------

#[test]
fn test_tls_module_import() {
    let v = eval(r#"import tls; tls"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_tls_connect_invalid_host() {
    // Connecting to a closed local port should fail at TCP, not panic.
    let err = eval_err(r#"import tls; tls.connect("127.0.0.1", 1)"#);
    assert!(err.contains("tls.connect"), "got: {}", err);
}

// ---------------------------------------------------------------------------
// ffi module (Phase 4.6 — real implementation via libloading)
// ---------------------------------------------------------------------------

#[test]
fn test_ffi_module_import() {
    let v = eval(r#"import ffi; ffi"#);
    assert!(matches!(v, Value::Module(_)));
}

#[test]
fn test_ffi_is_loaded_existing_file() {
    // Cargo.toml exists in the project root (cwd for tests).
    let v = eval(r#"import ffi; ffi.is_loaded("Cargo.toml")"#);
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn test_ffi_is_loaded_missing_file() {
    let v = eval(r#"import ffi; ffi.is_loaded("nonexistent_xyz_xyz.txt")"#);
    assert_eq!(v, Value::Bool(false));
}

#[test]
fn test_ffi_load_missing_library() {
    let err = eval_err(r#"import ffi; ffi.load("nonexistent_lib_xyz.so")"#);
    assert!(err.contains("ffi.load"), "got: {}", err);
}

#[test]
fn test_ffi_load_call_unload_libc() {
    // Load the C runtime library and call a harmless function from it.
    // On Windows that's msvcrt.dll; on Linux libc.so.6; on macOS libSystem.B.dylib.
    #[cfg(windows)]
    let src = r#"
        import ffi;
        let lib = ffi.load("msvcrt.dll");
        // abs(int) → int: call abs(-42) and expect 42.
        let r = ffi.call(lib, "abs", "int(int)", [-42]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "linux")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libc.so.6");
        // abs(int) → int: call abs(-42) and expect 42.
        let r = ffi.call(lib, "abs", "int(int)", [-42]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "macos")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libSystem.B.dylib");
        // abs(int) → int: call abs(-42) and expect 42.
        let r = ffi.call(lib, "abs", "int(int)", [-42]);
        ffi.unload(lib);
        r
    "#;
    let v = eval(src);
    assert_eq!(v, Value::int(42), "ffi abs(-42) should be 42");
}

#[test]
fn test_ffi_call_void_function() {
    // A void-returning function should yield Nil.
    #[cfg(windows)]
    let src = r#"
        import ffi;
        let lib = ffi.load("msvcrt.dll");
        // srand(uint) seeds the C PRNG; returns void.
        let r = ffi.call(lib, "srand", "void(uint)", [42]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "linux")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libc.so.6");
        let r = ffi.call(lib, "srand", "void(uint)", [42]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "macos")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libSystem.B.dylib");
        let r = ffi.call(lib, "srand", "void(uint)", [42]);
        ffi.unload(lib);
        r
    "#;
    let v = eval(src);
    assert_eq!(v, Value::Nil, "void function should return Nil");
}

#[test]
fn test_ffi_call_string_function() {
    // Call getenv: str(str) → returns a C string. We query "PATH" which is
    // virtually always set in any environment. We only assert that the result
    // is a non-empty Str (the exact value is platform/shell dependent).
    #[cfg(windows)]
    let src = r#"
        import ffi;
        let lib = ffi.load("msvcrt.dll");
        let r = ffi.call(lib, "getenv", "str(str)", ["PATH"]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "linux")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libc.so.6");
        let r = ffi.call(lib, "getenv", "str(str)", ["PATH"]);
        ffi.unload(lib);
        r
    "#;
    #[cfg(target_os = "macos")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libSystem.B.dylib");
        let r = ffi.call(lib, "getenv", "str(str)", ["PATH"]);
        ffi.unload(lib);
        r
    "#;
    let v = eval(src);
    match v {
        Value::Str(s) => assert!(!s.is_empty(), "PATH should be non-empty"),
        Value::Nil => panic!("getenv returned nil — PATH not visible to C runtime"),
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn test_ffi_call_after_unload_errors() {
    #[cfg(windows)]
    let src = r#"
        import ffi;
        let lib = ffi.load("msvcrt.dll");
        ffi.unload(lib);
        ffi.call(lib, "abs", "int(int)", [-1])
    "#;
    #[cfg(target_os = "linux")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libc.so.6");
        ffi.unload(lib);
        ffi.call(lib, "abs", "int(int)", [-1])
    "#;
    #[cfg(target_os = "macos")]
    let src = r#"
        import ffi;
        let lib = ffi.load("libSystem.B.dylib");
        ffi.unload(lib);
        ffi.call(lib, "abs", "int(int)", [-1])
    "#;
    let err = eval_err(src);
    assert!(err.contains("unloaded"), "got: {}", err);
}

#[test]
fn test_ffi_signature_parse_error() {
    let err = eval_err(r#"import ffi; ffi.load("msvcrt.dll"); ffi.call(ffi.load("msvcrt.dll"), "abs", "int[int]", [-1])"#);
    assert!(err.contains("ffi.call") && err.contains("signature"), "got: {}", err);
}

#[test]
fn test_ffi_arity_mismatch() {
    #[cfg(windows)]
    let src = r#"
        import ffi;
        let lib = ffi.load("msvcrt.dll");
        ffi.call(lib, "abs", "int(int)", [])
    "#;
    #[cfg(not(windows))]
    let src = r#"
        import ffi;
        let lib = ffi.load("libc.so.6");
        ffi.call(lib, "abs", "int(int)", [])
    "#;
    let err = eval_err(src);
    assert!(err.contains("expects 1 args, got 0"), "got: {}", err);
}
