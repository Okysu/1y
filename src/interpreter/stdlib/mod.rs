//! Standard library modules (Phase 4).
//!
//! Each submodule exposes a `build() -> ModuleRef` function that constructs a
//! `Value::Module` with native functions as exports. Modules are registered
//! once at interpreter startup and shared (via `Rc`) across all imports.

use crate::value::{ModuleData, ModuleRef};
use std::rc::Rc;
use std::collections::HashMap;

pub mod crypto;
pub mod env;
pub mod ffi;
pub mod io;
pub mod json;
pub mod process;
pub mod random;
pub mod serial;
pub mod socket;
pub mod tls;

/// Build all standard-library modules, returning a map keyed by module path.
///
/// Keys are dotted paths (e.g. `"io"`, `"env"`, `"json"`). `import io` looks
/// up `"io"` here before trying the file system.
pub fn build_std_modules() -> HashMap<String, ModuleRef> {
    let mut map = HashMap::new();
    map.insert("crypto".to_string(), crypto::build());
    map.insert("env".to_string(), env::build());
    map.insert("ffi".to_string(), ffi::build());
    map.insert("io".to_string(), io::build());
    map.insert("json".to_string(), json::build());
    map.insert("process".to_string(), process::build());
    map.insert("random".to_string(), random::build());
    map.insert("serial".to_string(), serial::build());
    map.insert("socket".to_string(), socket::build());
    map.insert("tls".to_string(), tls::build());
    map
}

/// Helper: construct a module from a list of `(name, NativeFn)` entries.
pub(crate) fn make_module(name: &str, entries: &[(&'static str, crate::value::NativeFn)]) -> ModuleRef {
    let mut exports = HashMap::new();
    for (n, nf) in entries {
        exports.insert((*n).to_string(), crate::value::Value::Native(std::rc::Rc::new(nf.clone())));
    }
    Rc::new(ModuleData {
        name: name.to_string(),
        source_path: None,
        exports,
    })
}
