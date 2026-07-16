//! Global actor registry for cross-thread actor messaging (Phase C1).
//!
//! In the BEAM-style concurrency model, each actor lives on a specific thread
//! (tied to that thread's `Scheduler` and coroutine pool). When an actor on
//! thread A wants to send a message to an actor on thread B, it needs a way
//! to address the target actor without holding a `!Send` `ActorRef` (`Rc`).
//!
//! The registry solves this with a two-level indirection:
//!
//! 1. **Global**: `ActorPid → Sender<CrossEnvelope>` — a thread-safe map
//!    from actor ID to a cross-thread channel sender. Any thread can look
//!    up a Pid and send a `CrossEnvelope` to the owning thread.
//!
//! 2. **Local (per-thread)**: `ActorPid → ActorRef` — each `Interpreter`
//!    maintains its own map of Pids to local `ActorRef`s. When a
//!    `CrossEnvelope` arrives on a thread, the receiver looks up the Pid
//!    in the local map and dispatches the message to the actor.
//!
//! Pids are allocated from a global atomic counter, guaranteeing uniqueness
//! across all threads.

use crate::value::{ActorPid, CrossEnvelope};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, Mutex};

/// Global monotonic counter for Pid allocation.
static PID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// The global registry: `ActorPid → cross-thread sender`.
static REGISTRY: LazyLock<Mutex<HashMap<u64, Sender<CrossEnvelope>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Allocate a fresh, globally-unique `ActorPid`.
pub fn allocate_pid() -> ActorPid {
    ActorPid(PID_COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Register a cross-thread sender for an actor.
///
/// Call this when an actor is spawned: the owning thread creates an
/// `mpsc::Receiver<CrossEnvelope>`, registers the `Sender` here, and polls
/// the receiver in its event loop.
pub fn register(pid: ActorPid, sender: Sender<CrossEnvelope>) {
    REGISTRY.lock().unwrap().insert(pid.0, sender);
}

/// Remove an actor from the global registry (e.g. when it is dropped).
pub fn unregister(pid: ActorPid) {
    REGISTRY.lock().unwrap().remove(&pid.0);
}

/// Look up the cross-thread sender for a Pid.
///
/// Returns `Some(sender)` if the actor is registered (possibly on another
/// thread), or `None` if no actor with that Pid exists.
pub fn get_sender(pid: ActorPid) -> Option<Sender<CrossEnvelope>> {
    REGISTRY.lock().unwrap().get(&pid.0).cloned()
}

/// Returns the number of currently registered actors (for diagnostics).
pub fn len() -> usize {
    REGISTRY.lock().unwrap().len()
}

/// Returns true if no actors are registered.
pub fn is_empty() -> bool {
    REGISTRY.lock().unwrap().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn test_pid_uniqueness() {
        let a = allocate_pid();
        let b = allocate_pid();
        assert_ne!(a, b, "Pids must be unique");
    }

    #[test]
    fn test_register_and_lookup() {
        let pid = allocate_pid();
        let (tx, _rx) = mpsc::channel::<CrossEnvelope>();
        register(pid, tx);
        assert!(get_sender(pid).is_some(), "sender should be registered");
        unregister(pid);
        assert!(get_sender(pid).is_none(), "sender should be removed");
    }

    #[test]
    fn test_send_across_threads() {
        use crate::value::SendValue;
        let pid = allocate_pid();
        let (tx, rx) = mpsc::channel::<CrossEnvelope>();
        register(pid, tx);

        // Send from this thread
        let env = CrossEnvelope {
            msg: SendValue::Int(42.into()),
            reply_slot: None,
        };
        get_sender(pid).unwrap().send(env).unwrap();

        // Receive (simulating another thread)
        let received = rx.recv().unwrap();
        match received.msg {
            SendValue::Int(n) => assert_eq!(n, 42.into()),
            _ => panic!("expected Int"),
        }

        unregister(pid);
    }
}
