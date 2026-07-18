//! VM closures and upvalues.
//!
//! Closures are the function values produced and invoked by the VM. Unlike
//! the tree-walker's `Closure` (which captures an entire `EnvRef` scope
//! chain), a VM closure captures only the outer variables it actually uses,
//! via **upvalues** — the Lua-style open/closed mechanism.
//!
//! # Upvalues
//!
//! An upvalue starts **open**, pointing at a live stack slot of the frame
//! that defined the closure. Reading/writing an open upvalue reads/writes
//! that slot directly. When the owning frame returns, its stack slots would
//! be invalidated — so the VM **closes** any open upvalues pointing into that
//! frame, moving the value out of the stack into the `Upvalue` itself
//! (`Closed`). From then on the upvalue owns the value on the "heap" (via
//! `Rc`).
//!
//! This gives correct lexical scoping with O(1) variable access and lets
//! closures outlive their defining function (e.g. a returned closure that
//! captures a local).

use crate::value::{ActorEnvVm, Value};
use crate::vm::chunk::Chunk;
use std::cell::RefCell;
use std::rc::Rc;

/// The state of an upvalue: either still pointing at a live stack slot, or
/// closed (owning its value) after the owning frame returned.
pub enum UpvalueState {
    /// Points at `stack[frame_base + slot]` of the frame whose stack region
    /// contains `slot`. We store the absolute stack index for direct access.
    Open { stack_index: usize },
    /// The owning frame has returned; the value now lives here.
    Closed(Value),
}

pub struct Upvalue {
    state: RefCell<UpvalueState>,
}

impl Upvalue {
    pub fn open(stack_index: usize) -> Self {
        Upvalue {
            state: RefCell::new(UpvalueState::Open { stack_index }),
        }
    }

    pub fn closed(v: Value) -> Self {
        Upvalue {
            state: RefCell::new(UpvalueState::Closed(v)),
        }
    }

    /// Read the upvalue's current value, borrowing from the value stack when
    /// still open.
    pub fn read<'s>(&self, stack: &'s [Value]) -> Value
    where
        Value: Clone,
    {
        match &*self.state.borrow() {
            UpvalueState::Open { stack_index } => stack[*stack_index].clone(),
            UpvalueState::Closed(v) => v.clone(),
        }
    }

    /// Write to the upvalue, mutating the live stack slot when open.
    pub fn write(&self, stack: &mut [Value], v: Value) {
        match &mut *self.state.borrow_mut() {
            UpvalueState::Open { stack_index } => stack[*stack_index] = v,
            UpvalueState::Closed(slot) => *slot = v,
        }
    }

    /// Close this upvalue if it is open and points at `stack_index`. The
    /// current stack value is moved into `Closed`.
    pub fn close_if_at(&self, stack: &[Value], stack_index: usize) -> bool {
        let mut st = self.state.borrow_mut();
        if let UpvalueState::Open { stack_index: idx } = &*st {
            if *idx == stack_index {
                let v = stack[*idx].clone();
                *st = UpvalueState::Closed(v);
                return true;
            }
        }
        false
    }

    /// Close this upvalue if it is open and points at any index `>= floor`.
    /// Used when a frame returns: every open upvalue at or above the frame's
    /// base must be closed.
    pub fn close_if_at_or_above(&self, stack: &[Value], floor: usize) -> bool {
        let mut st = self.state.borrow_mut();
        if let UpvalueState::Open { stack_index: idx } = &*st {
            if *idx >= floor {
                let v = stack[*idx].clone();
                *st = UpvalueState::Closed(v);
                return true;
            }
        }
        false
    }

    /// Borrow the upvalue's state (for sorted-insert position computation).
    pub fn state_ref(&self) -> std::cell::Ref<'_, UpvalueState> {
        self.state.borrow()
    }

    /// If open, return the stack index it points at; else `None`.
    pub fn stack_index(&self) -> Option<usize> {
        match &*self.state.borrow() {
            UpvalueState::Open { stack_index } => Some(*stack_index),
            UpvalueState::Closed(_) => None,
        }
    }
}

pub type UpvalueRef = Rc<Upvalue>;

/// A VM closure: a compiled chunk + the upvalues it captured.
#[derive(Clone)]
pub struct ClosureVm {
    pub chunk: Rc<Chunk>,
    pub upvalues: Vec<UpvalueRef>,
    pub name: Option<String>,
    pub arity: usize,
    /// Actor state namespace, if this closure belongs to an actor (handler
    /// or actor-local fn). `None` for module-level closures. Inherited from
    /// the creating frame at `OpCode::Closure` time.
    pub actor_env: Option<ActorEnvVm>,
}

impl ClosureVm {
    pub fn new(chunk: Rc<Chunk>, upvalues: Vec<UpvalueRef>) -> Self {
        let arity = chunk.arity;
        let name = chunk.name.clone();
        ClosureVm {
            chunk,
            upvalues,
            name,
            arity,
            actor_env: None,
        }
    }
}
