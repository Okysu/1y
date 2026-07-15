//! `ffi` module — Foreign Function Interface via dynamic library loading.
//!
//! Uses `libloading` for cross-platform `dlopen`/`dlsym`. The caller provides
//! a C-ABI signature string and arguments; the runtime transmutes the symbol
//! to the matching function pointer type and invokes it.
//!
//! Signature format:  `"ret(arg1, arg2, ...)"`
//!   - `void` → no return value (Nil)
//!   - `int`  → i64  (1y Int)
//!   - `uint` → u64  (1y Int)
//!   - `float`→ f64  (1y Decimal)
//!   - `str`  → *const c_char (1y Str; caller-allocated NUL-terminated for
//!             arguments; callee-owned NUL-terminated for returns)
//!
//! Up to 6 arguments are supported. This is enough for most libc functions.
//!
//! Safety: FFI is inherently unsafe. The runtime validates argument counts and
//! types against the signature, but cannot validate the foreign function's
//! memory accesses. Misuse can crash the interpreter. Only load trusted
//! libraries.
//!
//! Exports:
//!   - `load(path) -> Opaque`                 — open a shared library
//!   - `call(lib, name, sig, args) -> Value`  — invoke a foreign function
//!   - `unload(lib)`                          — close a loaded library
//!   - `is_loaded(path) -> Bool`              — check if a file exists at path
//!                                               (does NOT track loaded libs)

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, NativeResource, Value};
use libloading::Library;
use num_traits::FromPrimitive;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::rc::Rc;

pub fn build() -> ModuleRef {
    make_module(
        "ffi",
        &[
            ("load", NativeFn { name: "load", func: bi_load }),
            ("call", NativeFn { name: "call", func: bi_call }),
            ("unload", NativeFn { name: "unload", func: bi_unload }),
            ("is_loaded", NativeFn { name: "is_loaded", func: bi_is_loaded }),
        ],
    )
}

// --- helpers ---

fn str_arg(args: &[Value], idx: usize, name: &str) -> Result<String, InterpreterError> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok((**s).clone()),
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Str", got: v.type_name(), op: name.into(), span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    }
}

fn lib_at(args: &[Value], idx: usize, name: &str) -> Result<Rc<RefCell<Option<Library>>>, InterpreterError> {
    let r = match args.get(idx) {
        Some(Value::Opaque(r)) => r.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Opaque", got: v.type_name(), op: name.into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    };
    match &*r {
        NativeResource::SharedLib(l) => Ok(l.clone()),
        _ => Err(InterpreterError::TypeError {
            expected: "SharedLib", got: "opaque", op: name.into(), span: None,
        }),
    }
}

// --- ABI signature parsing ---

#[derive(Clone, Copy, PartialEq, Debug)]
enum AbiType {
    Void,
    Int,
    UInt,
    Float,
    Str,
}

impl AbiType {
    fn parse(s: &str) -> Result<AbiType, InterpreterError> {
        match s.trim() {
            "void" => Ok(AbiType::Void),
            "int" => Ok(AbiType::Int),
            "uint" => Ok(AbiType::UInt),
            "float" => Ok(AbiType::Float),
            "str" => Ok(AbiType::Str),
            other => Err(InterpreterError::RuntimeError {
                msg: format!("ffi.call: unknown ABI type `{}`", other),
                span: None,
            }),
        }
    }
}

struct Signature {
    ret: AbiType,
    args: Vec<AbiType>,
}

fn parse_sig(s: &str) -> Result<Signature, InterpreterError> {
    let s = s.trim();
    let open = s.find('(').ok_or_else(|| InterpreterError::RuntimeError {
        msg: format!("ffi.call: signature missing `(`: `{}`", s),
        span: None,
    })?;
    let close = s.rfind(')').ok_or_else(|| InterpreterError::RuntimeError {
        msg: format!("ffi.call: signature missing `)`: `{}`", s),
        span: None,
    })?;
    if close <= open {
        return Err(InterpreterError::RuntimeError {
            msg: format!("ffi.call: malformed signature: `{}`", s),
            span: None,
        });
    }
    let ret_str = &s[..open];
    let args_str = &s[open + 1..close];
    let ret = AbiType::parse(ret_str)?;
    let mut args = Vec::new();
    let inner = args_str.trim();
    if !inner.is_empty() {
        for part in inner.split(',') {
            args.push(AbiType::parse(part)?);
        }
    }
    if args.len() > 6 {
        return Err(InterpreterError::RuntimeError {
            msg: format!("ffi.call: too many arguments (max 6, got {})", args.len()),
            span: None,
        });
    }
    Ok(Signature { ret, args })
}

// --- argument conversion (1y Value → C ABI value) ---

// We bundle every argument into a `ArgSlot` (a tagged union of the C types we
// support) so the dispatch below can be uniform regardless of arity.
enum ArgSlot {
    I64(i64),
    U64(u64),
    F64(f64),
    Ptr(*const c_void),
}

// Keep CStrings alive for the duration of the call (the pointers we pass point
// into their buffers).
struct ArgKeeper {
    cstrings: Vec<CString>,
}

fn convert_arg(v: &Value, ty: AbiType, keeper: &mut ArgKeeper) -> Result<ArgSlot, InterpreterError> {
    match ty {
        AbiType::Int => match v {
            Value::Int(n) => {
                let i = n.to_i64().ok_or_else(|| InterpreterError::RuntimeError {
                    msg: "ffi.call: int argument out of i64 range".into(),
                    span: None,
                })?;
                Ok(ArgSlot::I64(i))
            }
            other => Err(InterpreterError::TypeError {
                expected: "Int", got: other.type_name(), op: "ffi.call".into(), span: None,
            }),
        },
        AbiType::UInt => match v {
            Value::Int(n) => {
                let u = n.to_u64().ok_or_else(|| InterpreterError::RuntimeError {
                    msg: "ffi.call: uint argument out of u64 range".into(),
                    span: None,
                })?;
                Ok(ArgSlot::U64(u))
            }
            other => Err(InterpreterError::TypeError {
                expected: "Int", got: other.type_name(), op: "ffi.call".into(), span: None,
            }),
        },
        AbiType::Float => match v {
            Value::Decimal(d) => {
                use num_traits::ToPrimitive;
                Ok(ArgSlot::F64(d.to_f64().unwrap_or(0.0)))
            }
            Value::Int(n) => {
                use num_traits::ToPrimitive;
                Ok(ArgSlot::F64(n.to_f64().unwrap_or(0.0)))
            }
            other => Err(InterpreterError::TypeError {
                expected: "Decimal or Int", got: other.type_name(), op: "ffi.call".into(), span: None,
            }),
        },
        AbiType::Str => match v {
            Value::Str(s) => {
                let c = CString::new(s.as_str()).map_err(|_| InterpreterError::RuntimeError {
                    msg: "ffi.call: string contains interior NUL byte".into(),
                    span: None,
                })?;
                let p = c.as_ptr() as *const c_void;
                keeper.cstrings.push(c);
                Ok(ArgSlot::Ptr(p))
            }
            Value::Nil => Ok(ArgSlot::Ptr(std::ptr::null())),
            other => Err(InterpreterError::TypeError {
                expected: "Str or Nil", got: other.type_name(), op: "ffi.call".into(), span: None,
            }),
        },
        AbiType::Void => Err(InterpreterError::RuntimeError {
            msg: "ffi.call: `void` is not a valid argument type".into(),
            span: None,
        }),
    }
}

use num_traits::ToPrimitive;

// --- return value conversion (C ABI value → 1y Value) ---

unsafe fn cstr_to_value(p: *const c_char) -> Value {
    if p.is_null() {
        return Value::Nil;
    }
    match CStr::from_ptr(p).to_str() {
        Ok(s) => Value::str(s),
        Err(_) => Value::str(String::from_utf8_lossy(CStr::from_ptr(p).to_bytes())),
    }
}

// --- the dispatch: one branch per arity ---

// We inline each arity rather than using a macro, because the macro would need
// to repeat the `*const c_void` parameter list, and Rust's macro repetition
// requires at least one syntax variable in the repeated fragment.

fn convert_ret(ret: AbiType, raw: usize) -> Result<Value, InterpreterError> {
    match ret {
        AbiType::Void => Ok(Value::Nil),
        AbiType::Int => {
            // Sign-extend the low 64 bits.
            let i = raw as i64;
            Ok(Value::int(i))
        }
        AbiType::UInt => {
            // On 64-bit platforms raw already holds the full u64; on 32-bit we
            // only have usize bits. This implementation assumes 64-bit.
            let u = raw as u64;
            Ok(Value::int(u as i64))
        }
        AbiType::Float => {
            // Reinterpret the usize bits as f64. This is safe because we
            // transmuted the return through usize on a 64-bit platform.
            let f = f64::from_bits(raw as u64);
            match bigdecimal::BigDecimal::from_f64(f) {
                Some(d) => Ok(Value::Decimal(d)),
                None => Err(InterpreterError::RuntimeError {
                    msg: "ffi.call: float return conversion failed".into(),
                    span: None,
                }),
            }
        }
        AbiType::Str => {
            let p = raw as *const c_char;
            // Safety: the callee must return a valid NUL-terminated C string.
            unsafe { Ok(cstr_to_value(p)) }
        }
    }
}

// Cast an ArgSlot to a *const c_void for the transmute call.
fn slot_to_ptr(slot: &ArgSlot) -> *const c_void {
    match slot {
        ArgSlot::I64(i) => *i as *const c_void,
        ArgSlot::U64(u) => *u as usize as *const c_void,
        ArgSlot::F64(f) => f.to_bits() as usize as *const c_void,
        ArgSlot::Ptr(p) => *p,
    }
}

// --- public functions ---

fn bi_load(args: &[Value]) -> Result<Value, InterpreterError> {
    let path = str_arg(args, 0, "ffi.load")?;
    let lib = unsafe { Library::new(&path) }.map_err(|e| InterpreterError::RuntimeError {
        msg: format!("ffi.load: `{}`: {}", path, e),
        span: None,
    })?;
    Ok(Value::Opaque(Rc::new(NativeResource::SharedLib(Rc::new(RefCell::new(Some(lib)))))))
}

fn bi_call(args: &[Value]) -> Result<Value, InterpreterError> {
    // call(lib, name, sig, args_vec)
    if args.len() < 3 {
        return Err(InterpreterError::ArityError {
            expected: 3, got: args.len(), callee: "ffi.call".into(), span: None,
        });
    }
    let lib_rc = lib_at(args, 0, "ffi.call")?;
    let name = str_arg(args, 1, "ffi.call")?;
    let sig_str = str_arg(args, 2, "ffi.call")?;
    let sig = parse_sig(&sig_str)?;

    // The 4th argument is a Vec of call arguments.
    let call_args: Vec<Value> = match &args[3] {
        Value::Vec(v) => v.iter().cloned().collect(),
        Value::Nil => Vec::new(),
        other => return Err(InterpreterError::TypeError {
            expected: "Vec or Nil", got: other.type_name(), op: "ffi.call".into(), span: None,
        }),
    };
    if call_args.len() != sig.args.len() {
        return Err(InterpreterError::RuntimeError {
            msg: format!(
                "ffi.call: signature `{}` expects {} args, got {}",
                sig_str, sig.args.len(), call_args.len()
            ),
            span: None,
        });
    }

    // Convert args.
    let mut keeper = ArgKeeper { cstrings: Vec::new() };
    let mut slots: Vec<ArgSlot> = Vec::with_capacity(call_args.len());
    for (v, ty) in call_args.iter().zip(sig.args.iter()) {
        slots.push(convert_arg(v, *ty, &mut keeper)?);
    }

    // Borrow the library. We need to keep the RefCell borrow active for the
    // duration of the call.
    let lib_ref = lib_rc.borrow();
    let lib = lib_ref.as_ref().ok_or_else(|| InterpreterError::RuntimeError {
        msg: "ffi.call: library has been unloaded".into(),
        span: None,
    })?;

    // Dispatch by arity. We pass *const c_void for every slot; the transmute
    // in `call_arity!` reinterprets the symbol as a function taking that many
    // `*const c_void` args and returning usize.
    //
    // SAFETY: the caller asserts that `name` names a function with a C ABI
    // matching `sig`. We transmute the symbol to a concrete fn pointer type
    // and call it. Argument widths (i64/u64/f64/*const c_void) are all 8 bytes
    // on 64-bit platforms, so passing them through `*const c_void` slots is
    // bit-preserving.
    let result = unsafe {
        match slots.len() {
            0 => {
                let sym: libloading::Symbol<unsafe extern "C" fn() -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym())
            }
            1 => {
                let a = slot_to_ptr(&slots[0]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a))
            }
            2 => {
                let a0 = slot_to_ptr(&slots[0]);
                let a1 = slot_to_ptr(&slots[1]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a0, a1))
            }
            3 => {
                let a0 = slot_to_ptr(&slots[0]);
                let a1 = slot_to_ptr(&slots[1]);
                let a2 = slot_to_ptr(&slots[2]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a0, a1, a2))
            }
            4 => {
                let a0 = slot_to_ptr(&slots[0]);
                let a1 = slot_to_ptr(&slots[1]);
                let a2 = slot_to_ptr(&slots[2]);
                let a3 = slot_to_ptr(&slots[3]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a0, a1, a2, a3))
            }
            5 => {
                let a0 = slot_to_ptr(&slots[0]);
                let a1 = slot_to_ptr(&slots[1]);
                let a2 = slot_to_ptr(&slots[2]);
                let a3 = slot_to_ptr(&slots[3]);
                let a4 = slot_to_ptr(&slots[4]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *const c_void, *const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a0, a1, a2, a3, a4))
            }
            6 => {
                let a0 = slot_to_ptr(&slots[0]);
                let a1 = slot_to_ptr(&slots[1]);
                let a2 = slot_to_ptr(&slots[2]);
                let a3 = slot_to_ptr(&slots[3]);
                let a4 = slot_to_ptr(&slots[4]);
                let a5 = slot_to_ptr(&slots[5]);
                let sym: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, *const c_void, *const c_void, *const c_void) -> usize> =
                    lib.get(name.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                        msg: format!("ffi.call: symbol `{}`: {}", name, e),
                        span: None,
                    })?;
                convert_ret(sig.ret, sym(a0, a1, a2, a3, a4, a5))
            }
            n => return Err(InterpreterError::RuntimeError {
                msg: format!("ffi.call: unsupported arity {} (max 6)", n),
                span: None,
            }),
        }
    };
    result
}

fn bi_unload(args: &[Value]) -> Result<Value, InterpreterError> {
    let lib_rc = lib_at(args, 0, "ffi.unload")?;
    let mut slot = lib_rc.borrow_mut();
    if slot.is_none() {
        return Err(InterpreterError::RuntimeError {
            msg: "ffi.unload: library already unloaded".into(),
            span: None,
        });
    }
    // Drop the Library by replacing with None.
    *slot = None;
    Ok(Value::Nil)
}

fn bi_is_loaded(args: &[Value]) -> Result<Value, InterpreterError> {
    // Check if a file exists at the given path. This is a lightweight probe —
    // it does NOT track which libraries are currently loaded by `ffi.load`.
    let path = str_arg(args, 0, "ffi.is_loaded")?;
    Ok(Value::Bool(Path::new(&path).exists()))
}
