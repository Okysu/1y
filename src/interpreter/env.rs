//! Lexical environment (scope chain) for the tree-walking interpreter.
//!
//! Each environment is a frame of name→value bindings with an optional parent.
//! Closures capture their defining environment, giving us lexical scoping.
//!
//! `Rc<RefCell<Environment>>` allows shared mutation: `let` inserts into the
//! current frame, `=` (assignment) walks up the chain to find and replace an
//! existing binding.

use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A reference to a shared, mutable environment frame.
pub type EnvRef = Rc<RefCell<Environment>>;

/// One frame in the scope chain.
#[derive(Debug)]
pub struct Environment {
    bindings: HashMap<String, Value>,
    parent: Option<EnvRef>,
}

impl Environment {
    pub fn new(parent: Option<EnvRef>) -> EnvRef {
        Rc::new(RefCell::new(Environment {
            bindings: HashMap::new(),
            parent,
        }))
    }

    /// Create a fresh top-level (global) environment.
    pub fn global() -> EnvRef {
        Self::new(None)
    }

    /// Create a child frame of `parent` (for function calls / block scopes).
    pub fn child(parent: &EnvRef) -> EnvRef {
        Self::new(Some(Rc::clone(parent)))
    }

    /// Define a new binding in *this* frame (shadows parent).
    pub fn define(&mut self, name: impl Into<String>, value: Value) {
        self.bindings.insert(name.into(), value);
    }

    /// Look up `name`, walking up the scope chain. Returns `None` if unbound.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.bindings.get(name) {
            return Some(v.clone());
        }
        self.parent.as_ref().and_then(|p| p.borrow().get(name))
    }

    /// Assign to an *existing* binding (walks up the chain). Returns `false`
    /// if the name is not bound anywhere (caller should raise a NameError).
    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        if self.bindings.contains_key(name) {
            self.bindings.insert(name.to_string(), value);
            true
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().assign(name, value)
        } else {
            false
        }
    }

    /// Check if `name` is bound anywhere in the chain.
    pub fn has(&self, name: &str) -> bool {
        if self.bindings.contains_key(name) {
            true
        } else if let Some(parent) = &self.parent {
            parent.borrow().has(name)
        } else {
            false
        }
    }

    /// Clone all bindings defined in *this* frame (not parent frames).
    /// Used by or-patterns to transfer bindings from a trial sub-environment.
    pub fn bindings_clone(&self) -> HashMap<String, Value> {
        self.bindings.clone()
    }
}
