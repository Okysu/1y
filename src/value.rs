//! Runtime value types for the `1y` interpreter.
//!
//! A [`Value`] is what every expression evaluates to. The design follows the
//! language's core principle of "numerical unification": integers are arbitrary
//! precision ([`BigInt`]) and decimals are arbitrary precision
//! ([`BigDecimal`]); arithmetic auto-promotes but the two remain distinct
//! types for equality / map-key purposes.
//!
//! Persistent collections ([`im::Vector`], [`im::HashMap`], [`im::HashSet`])
//! provide structural sharing: "modifying" a collection returns
//! a new version that shares most of its internals with the old one.

use crate::ast::{Expr, OnClause, Param};
use bigdecimal::BigDecimal;
use im::{HashMap as ImMap, HashSet as ImSet, Vector as ImVector};
use num_bigint::BigInt;
use std::cell::RefCell;
use std::collections::{HashMap as StdMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;

use crate::interpreter::env::EnvRef;

// ---------------------------------------------------------------------------
// Closure / native function
// ---------------------------------------------------------------------------

/// A user-defined function (closure): captured environment + parameters + body.
#[derive(Clone)]
pub struct Closure {
    pub params: Vec<Param>,
    pub body: Expr,
    pub env: EnvRef,
    pub name: Option<String>,
}

/// A built-in function implemented in Rust.
pub type NativeFnPtr = fn(&[Value]) -> Result<Value, crate::interpreter::error::InterpreterError>;

#[derive(Clone)]
pub struct NativeFn {
    pub name: &'static str,
    pub func: NativeFnPtr,
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

/// A pending message in an actor's mailbox.
///
/// For `!` (fire-and-forget) `reply` is `None`. For `?` (request/reply) it is
/// a shared slot that the handler fills via `reply expr`.
#[derive(Clone)]
pub struct Envelope {
    /// The message value — conventionally a `Value::Variant` whose name matches
    /// an `on` handler, but any value may be sent.
    pub msg: Value,
    /// For `?`: a slot to receive the handler's reply. `None` for `!`.
    pub reply_slot: Option<Rc<RefCell<Option<Value>>>>,
}

/// A `Send` version of [`Envelope`] for cross-thread actor messaging.
///
/// When an actor on thread A sends a message to an actor on thread B, the
/// `Value` message payload is converted to [`SendValue`] (erroring on
/// non-Send types like functions or resources), and the `reply_slot` uses
/// `Arc<Mutex<…>>` instead of `Rc<RefCell<…>>` so the reply can cross
/// thread boundaries.
///
/// The receiving thread converts the `SendValue` back to a `Value` and
/// dispatches it to the local actor, filling the reply slot when done.
pub struct CrossEnvelope {
    /// The message payload, as a `Send` data-only subset of `Value`.
    pub msg: SendValue,
    /// For `?` (request/reply): a cross-thread reply slot. `None` for `!`.
    pub reply_slot: Option<std::sync::Arc<std::sync::Mutex<Option<SendValue>>>>,
}

// Compile-time assertion that CrossEnvelope is Send + Sync.
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    assert_send::<CrossEnvelope>();
    assert_sync::<CrossEnvelope>();
    assert_send::<SendValue>();
    assert_sync::<SendValue>();
    assert_send::<ActorPid>();
};

/// An actor instance: isolated state + message handlers + mailbox.
///
/// Actors run on a single-threaded event loop. `!` enqueues a message and
/// returns immediately; `?` invokes the matching handler synchronously and
/// waits for `reply`. The event loop drains pending `!` messages when the
/// main program finishes (or when explicitly flushed).
pub struct ActorInstance {
    /// Globally-unique actor ID (Phase C3). Allocated at spawn time and
    /// registered in the global `ActorRegistry` so actors on other threads
    /// can route messages to this actor via `CrossEnvelope`.
    pub pid: ActorPid,
    /// Environment holding the actor's `state` bindings (parent = global).
    pub env: EnvRef,
    /// `on name(params)` handlers, keyed by handler name.
    pub handlers: HashMap<String, OnClause>,
    /// Pending incoming messages (FIFO).
    pub mailbox: VecDeque<Envelope>,
}

impl ActorInstance {
    pub fn new(env: EnvRef) -> Self {
        ActorInstance {
            pid: crate::runtime::registry::allocate_pid(),
            env,
            handlers: HashMap::new(),
            mailbox: VecDeque::new(),
        }
    }
}

/// A reference to a shared, mutable actor instance.
pub type ActorRef = Rc<RefCell<ActorInstance>>;

// ---------------------------------------------------------------------------
// Shared (transactional) cell — Phase 3 MVCC
// ---------------------------------------------------------------------------

/// A versioned mutable cell, created by `shared name = value`.
///
/// Reads/writes inside a `transact { ... }` block are buffered in a
/// transaction-local write-set and committed atomically on success (bumping
/// the version) or discarded on failure. Outside a transaction, reads return
/// the current value and writes update it directly (bumping the version).
pub struct SharedCell {
    /// The current committed value.
    pub value: Value,
    /// Monotonically increasing version, bumped on each direct commit.
    pub version: u64,
}

/// A reference to a versioned shared cell.
pub type SharedRef = Rc<RefCell<SharedCell>>;

// ---------------------------------------------------------------------------
// Native resources (Phase 4) — opaque handles for I/O objects
// ---------------------------------------------------------------------------

/// An opaque native resource: file handles, TCP sockets, serial ports, etc.
///
/// These are created by standard-library functions (e.g. `socket.listen`)
/// and passed back to other stdlib functions (e.g. `socket.accept`). They
/// cannot be inspected or constructed from `1y` code.
pub enum NativeResource {
    /// A TCP listener (server socket), created by `socket.listen`.
    TcpListener(Rc<RefCell<std::net::TcpListener>>),
    /// A TCP stream (client connection), returned by `socket.accept`.
    TcpStream(Rc<RefCell<std::net::TcpStream>>),
    /// A serial port, created by `serial.open`.
    Serial(Rc<RefCell<Box<dyn serialport::SerialPort>>>),
    /// A TLS client stream, created by `tls.connect`.
    TlsStream(Rc<RefCell<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>>>),
    /// A loaded shared library, created by `ffi.load`.
    /// The `OnceLock` wrapper is a workaround: `libloading::Library` is not
    /// `Clone`, so we share a single instance behind `Rc<RefCell<...>>`. The
    /// outer `OnceLock` is never re-set; it only exists to satisfy the
    /// `Clone` derive on the outer `Rc` clone (we never actually clone the
    /// Library itself).
    SharedLib(Rc<RefCell<Option<libloading::Library>>>),
}

impl Clone for NativeResource {
    fn clone(&self) -> Self {
        match self {
            NativeResource::TcpListener(r) => NativeResource::TcpListener(r.clone()),
            NativeResource::TcpStream(r) => NativeResource::TcpStream(r.clone()),
            NativeResource::Serial(r) => NativeResource::Serial(r.clone()),
            NativeResource::TlsStream(r) => NativeResource::TlsStream(r.clone()),
            NativeResource::SharedLib(r) => NativeResource::SharedLib(r.clone()),
        }
    }
}

/// A reference to a native resource.
pub type ResourceRef = Rc<NativeResource>;

// ---------------------------------------------------------------------------
// Async Task (Phase 4.7: Zig-style colorless async)
// ---------------------------------------------------------------------------

/// The result of polling a [`Task`].
pub enum TaskPoll {
    /// The task has completed with this value.
    Ready(Value),
    /// The task is not ready yet; poll again later.
    Pending,
}

/// Internal state of a Task.
pub enum TaskState {
    /// Pending: holds a poll function that checks if the underlying operation
    /// (timer, I/O) has completed.
    Pending(Box<dyn Fn() -> TaskPoll>),
    /// Ready: the task has completed; the value can be extracted by `await`.
    Ready(Value),
    /// Consumed: the value has already been extracted by `await`.
    /// Polling or awaiting a consumed task is an error.
    Consumed,
}

/// A shared, mutable async task.
pub type TaskRef = Rc<RefCell<TaskState>>;

// ---------------------------------------------------------------------------
// Module (Phase 4)
// ---------------------------------------------------------------------------

/// A loaded module: its exported bindings keyed by name.
///
/// Standard-library modules (io, env, json, ...) are built from native
/// functions. User modules are built by evaluating a `.1y` source file and
/// collecting its top-level bindings.
#[derive(Clone)]
pub struct ModuleData {
    /// Module name or file path (for diagnostics).
    pub name: String,
    /// The canonicalized source path if this is a file module (`None` for std).
    pub source_path: Option<PathBuf>,
    /// Exported name → value.
    pub exports: StdMap<String, Value>,
}

/// A reference to a module (shared so that `import` is cheap to clone).
pub type ModuleRef = Rc<ModuleData>;

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// A runtime value. Cheap to clone — collections use Arc-based structural
/// sharing, strings are reference-counted.
#[derive(Clone)]
pub enum Value {
    // --- numbers ---
    Int(BigInt),
    Decimal(BigDecimal),

    // --- primitives ---
    Str(Rc<String>),
    Bool(bool),
    Nil,

    // --- persistent collections ---
    Vec(ImVector<Value>),
    Map(ImMap<Value, Value>),
    Set(ImSet<Value>),

    // --- functions ---
    Func(Rc<Closure>),
    Native(Rc<NativeFn>),

    // --- user-defined types ---
    /// An enum variant instance, e.g. `Some(42)` → `Variant { name: "Some", args: [42] }`.
    Variant {
        name: Rc<String>,
        args: Rc<Vec<Value>>,
    },
    /// A struct instance, e.g. `Point { x: 1, y: 2 }`.
    Struct {
        name: Rc<String>,
        fields: Rc<StdMap<String, Value>>,
    },

    // --- concurrency ---
    /// An actor instance, created by `spawn Name(args)`.
    Actor(ActorRef),
    /// A versioned shared cell, created by `shared name = value`.
    Shared(SharedRef),

    // --- modules & resources (Phase 4) ---
    /// A loaded module, created by `import`.
    Module(ModuleRef),
    /// An opaque native resource (file handle, socket, etc.).
    Opaque(ResourceRef),
    /// A deferred import — loaded on first access.
    LazyImport { path: Rc<String> },

    // --- async (Phase 4.7: Zig-style colorless async) ---
    /// A pending async task, created by functions like `socket.read_async`.
    /// `await` suspends the current coroutine until the task is ready, then
    /// extracts the inner value. Tasks are single-use.
    Task(TaskRef),
}

impl Value {
    // --- constructors ---

    pub fn int(n: impl Into<BigInt>) -> Self {
        Value::Int(n.into())
    }

    pub fn str(s: impl Into<String>) -> Self {
        Value::Str(Rc::new(s.into()))
    }

    pub fn vec(items: Vec<Value>) -> Self {
        Value::Vec(items.into_iter().collect())
    }

    pub fn map(entries: Vec<(Value, Value)>) -> Self {
        Value::Map(entries.into_iter().collect())
    }

    pub fn set(items: Vec<Value>) -> Self {
        Value::Set(items.into_iter().collect())
    }

    // --- type predicates ---

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Nil => false,
            Value::Int(n) => n != &BigInt::from(0),
            Value::Decimal(d) => d != &BigDecimal::from(0),
            Value::Str(s) => !s.is_empty(),
            Value::Vec(v) => !v.is_empty(),
            Value::Map(m) => !m.is_empty(),
            Value::Set(s) => !s.is_empty(),
            _ => true, // functions, variants, structs are truthy
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Decimal(_) => "Decimal",
            Value::Str(_) => "String",
            Value::Bool(_) => "Bool",
            Value::Nil => "Nil",
            Value::Vec(_) => "Vec",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Func(_) => "Func",
            Value::Native(_) => "NativeFunc",
            Value::Variant { .. } => "Variant",
            Value::Struct { .. } => "Struct",
            Value::Actor(_) => "Actor",
            Value::Shared(_) => "Shared",
            Value::Module(_) => "Module",
            Value::Opaque(_) => "Opaque",
            Value::LazyImport { .. } => "LazyImport",
            Value::Task(_) => "Task",
        }
    }

    /// True if this value is a number (Int or Decimal).
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Decimal(_))
    }

    /// Convert to a BigDecimal for mixed arithmetic promotion.
    pub fn to_decimal(&self) -> Option<BigDecimal> {
        match self {
            Value::Int(n) => Some(BigDecimal::from(n.clone())),
            Value::Decimal(d) => Some(d.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Equality — structural for data, identity for functions.
// ---------------------------------------------------------------------------

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Decimal(a), Value::Decimal(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Vec(a), Value::Vec(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (
                Value::Variant { name: an, args: aa },
                Value::Variant { name: bn, args: ba },
            ) => an == bn && aa == ba,
            (
                Value::Struct { name: an, fields: af },
                Value::Struct { name: bn, fields: bf },
            ) => an == bn && af == bf,
            // Functions: identity (same Rc).
            (Value::Func(a), Value::Func(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => a.name == b.name,
            // Actors: identity (same Rc).
            (Value::Actor(a), Value::Actor(b)) => Rc::ptr_eq(a, b),
            // Shared cells: identity (same Rc).
            (Value::Shared(a), Value::Shared(b)) => Rc::ptr_eq(a, b),
            // Modules: identity (same Rc).
            (Value::Module(a), Value::Module(b)) => Rc::ptr_eq(a, b),
            // Opaque resources: identity (same Rc).
            (Value::Opaque(a), Value::Opaque(b)) => Rc::ptr_eq(a, b),
            // Lazy imports: equal if same path.
            (
                Value::LazyImport { path: a },
                Value::LazyImport { path: b },
            ) => a == b,
            // Tasks: identity (same Rc).
            (Value::Task(a), Value::Task(b)) => Rc::ptr_eq(a, b),
            // Different type tags are never equal.
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Discriminant byte prevents cross-type hash collisions
        // (e.g. Int(1) vs Bool(true)).
        match self {
            Value::Int(n) => {
                0u8.hash(state);
                n.hash(state);
            }
            Value::Decimal(d) => {
                1u8.hash(state);
                d.hash(state);
            }
            Value::Str(s) => {
                2u8.hash(state);
                s.hash(state);
            }
            Value::Bool(b) => {
                3u8.hash(state);
                b.hash(state);
            }
            Value::Nil => {
                4u8.hash(state);
            }
            Value::Vec(v) => {
                5u8.hash(state);
                v.hash(state);
            }
            Value::Map(m) => {
                6u8.hash(state);
                m.hash(state);
            }
            Value::Set(s) => {
                7u8.hash(state);
                s.hash(state);
            }
            Value::Variant { name, args } => {
                9u8.hash(state);
                name.hash(state);
                args.hash(state);
            }
            Value::Struct { name, fields } => {
                10u8.hash(state);
                name.hash(state);
                // Hash field entries in a deterministic order: we cannot rely
                // on HashMap iteration order, so sort by key first.
                let mut keys: Vec<_> = fields.keys().collect();
                keys.sort();
                for k in keys {
                    k.hash(state);
                    fields[k].hash(state);
                }
            }
            Value::Func(f) => {
                11u8.hash(state);
                (Rc::as_ptr(f) as usize).hash(state);
            }
            Value::Native(n) => {
                12u8.hash(state);
                n.name.hash(state);
            }
            Value::Actor(a) => {
                13u8.hash(state);
                (Rc::as_ptr(a) as usize).hash(state);
            }
            Value::Shared(s) => {
                14u8.hash(state);
                (Rc::as_ptr(s) as usize).hash(state);
            }
            Value::Module(m) => {
                15u8.hash(state);
                (Rc::as_ptr(m) as usize).hash(state);
            }
            Value::Opaque(r) => {
                16u8.hash(state);
                (Rc::as_ptr(r) as usize).hash(state);
            }
            Value::LazyImport { path } => {
                17u8.hash(state);
                path.hash(state);
            }
            Value::Task(t) => {
                18u8.hash(state);
                Rc::as_ptr(t).hash(state);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Decimal(d) => write!(f, "{}", d),
            Value::Str(s) => write!(f, "\"{}\"", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Nil => write!(f, "nil"),
            Value::Vec(v) => {
                write!(f, "[")?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Set(s) => {
                write!(f, "#{{")?;
                for (i, item) in s.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            Value::Func(c) => match &c.name {
                Some(n) => write!(f, "<fn {}>", n),
                None => write!(f, "<fn>"),
            },
            Value::Native(n) => write!(f, "<builtin {}>", n.name),
            Value::Variant { name, args } => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}(", name)?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", a)?;
                    }
                    write!(f, ")")
                }
            }
            Value::Struct { name, fields } => {
                write!(f, "{} {{ ", name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Actor(_) => write!(f, "<actor>"),
            Value::Shared(s) => write!(f, "<shared v{}>", s.borrow().version),
            Value::Module(m) => write!(f, "<module {}>", m.name),
            Value::Opaque(r) => match &**r {
                NativeResource::TcpListener(_) => write!(f, "<tcp-listener>"),
                NativeResource::TcpStream(_) => write!(f, "<tcp-stream>"),
                NativeResource::Serial(_) => write!(f, "<serial-port>"),
                NativeResource::TlsStream(_) => write!(f, "<tls-stream>"),
                NativeResource::SharedLib(_) => write!(f, "<shared-lib>"),
            },
            Value::LazyImport { path } => write!(f, "<lazy-import {}>", path),
            Value::Task(_) => write!(f, "<task>"),
        }
    }
}

// ---------------------------------------------------------------------------
// ActorPid — lightweight Send identifier for cross-thread actor messaging
// ---------------------------------------------------------------------------

/// A lightweight, `Send` + `Copy` identifier for an actor.
///
/// In the BEAM-style concurrency model, actors live on a specific thread
/// (tied to that thread's `Scheduler` and coroutine pool). When an actor
/// reference needs to cross thread boundaries, it is represented as a `Pid`
/// — a globally unique `u64` — rather than the `!Send` `ActorRef` (`Rc`).
///
/// The receiving thread resolves the `Pid` back to a local `ActorRef` via
/// the global `ActorRegistry` (see `runtime/registry.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActorPid(pub u64);

impl std::fmt::Display for ActorPid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<pid {}>", self.0)
    }
}

// ---------------------------------------------------------------------------
// SendValue — a Send subset of Value for cross-thread actor messages
// ---------------------------------------------------------------------------

/// A `Send` subset of [`Value`] that can safely cross thread boundaries.
///
/// In the BEAM-style concurrency model, only **data** values can be sent in
/// cross-thread Actor messages — not functions, closures, shared cells,
/// tasks, or I/O resources (all of which are `!Send` due to `Rc`).
///
/// `SendValue` is the wire format for cross-thread `Envelope`s. The sending
/// thread calls [`SendValue::from_value`] to convert a `Value` into a
/// `SendValue` (erroring on non-Send types), and the receiving thread calls
/// [`SendValue::into_value`] to reconstruct a `Value`.
///
/// `ActorPid` is included so actor references can be forwarded across threads.
#[derive(Clone, Debug)]
pub enum SendValue {
    // --- numbers ---
    Int(BigInt),
    Decimal(BigDecimal),

    // --- primitives ---
    Str(String),
    Bool(bool),
    Nil,

    // --- persistent collections (im is Send + Sync when elements are) ---
    Vec(ImVector<SendValue>),
    Map(ImMap<SendValue, SendValue>),
    Set(ImSet<SendValue>),

    // --- user-defined types ---
    Variant {
        name: String,
        args: Vec<SendValue>,
    },
    Struct {
        name: String,
        fields: StdMap<String, SendValue>,
    },

    // --- cross-thread actor reference ---
    /// An actor on another thread, identified by Pid.
    ActorPid(ActorPid),
}

impl SendValue {
    /// Convert a `Value` to a `SendValue`, returning an error message for
    /// types that cannot cross thread boundaries (functions, shared cells,
    /// tasks, resources, etc.).
    pub fn from_value(v: &Value) -> Result<Self, String> {
        match v {
            Value::Int(n) => Ok(SendValue::Int(n.clone())),
            Value::Decimal(d) => Ok(SendValue::Decimal(d.clone())),
            Value::Str(s) => Ok(SendValue::Str((**s).clone())),
            Value::Bool(b) => Ok(SendValue::Bool(*b)),
            Value::Nil => Ok(SendValue::Nil),
            Value::Vec(v) => {
                let items: Result<ImVector<_>, _> =
                    v.iter().map(SendValue::from_value).collect();
                Ok(SendValue::Vec(items?))
            }
            Value::Map(m) => {
                let mut map = ImMap::new();
                for (k, v) in m.iter() {
                    map.insert(SendValue::from_value(k)?, SendValue::from_value(v)?);
                }
                Ok(SendValue::Map(map))
            }
            Value::Set(s) => {
                let mut set = ImSet::new();
                for item in s.iter() {
                    set.insert(SendValue::from_value(item)?);
                }
                Ok(SendValue::Set(set))
            }
            Value::Variant { name, args } => {
                let args: Result<Vec<_>, _> =
                    args.iter().map(SendValue::from_value).collect();
                Ok(SendValue::Variant {
                    name: (**name).clone(),
                    args: args?,
                })
            }
            Value::Struct { name, fields } => {
                let mut out_fields = StdMap::new();
                for (k, v) in fields.iter() {
                    out_fields.insert(k.clone(), SendValue::from_value(v)?);
                }
                Ok(SendValue::Struct {
                    name: (**name).clone(),
                    fields: out_fields,
                })
            }
            Value::Actor(_) => {
                Err("cannot send Actor reference across threads (use a Pid)".into())
            }
            Value::Func(_) => Err("cannot send function across threads".into()),
            Value::Native(_) => Err("cannot send native function across threads".into()),
            Value::Shared(_) => Err("cannot send shared cell across threads".into()),
            Value::Module(_) => Err("cannot send module across threads".into()),
            Value::Opaque(_) => Err("cannot send resource across threads".into()),
            Value::LazyImport { .. } => {
                Err("cannot send lazy import across threads".into())
            }
            Value::Task(_) => Err("cannot send task across threads".into()),
        }
    }

    /// Convert a `SendValue` back to a `Value` on the receiving thread.
    ///
    /// `ActorPid` values are **not** resolved here — the caller must look up
    /// the Pid in the `ActorRegistry` and replace the `Nil` placeholder with
    /// a local `Value::Actor` if the actor lives on this thread, or keep it
    /// as a Pid-valued placeholder if it lives on another thread.
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Int(n) => Value::Int(n),
            SendValue::Decimal(d) => Value::Decimal(d),
            SendValue::Str(s) => Value::Str(Rc::new(s)),
            SendValue::Bool(b) => Value::Bool(b),
            SendValue::Nil => Value::Nil,
            SendValue::Vec(v) => {
                Value::Vec(v.into_iter().map(SendValue::into_value).collect())
            }
            SendValue::Map(m) => Value::Map(
                m.into_iter()
                    .map(|(k, v)| (k.into_value(), v.into_value()))
                    .collect(),
            ),
            SendValue::Set(s) => {
                Value::Set(s.into_iter().map(SendValue::into_value).collect())
            }
            SendValue::Variant { name, args } => Value::Variant {
                name: Rc::new(name),
                args: Rc::new(args.into_iter().map(SendValue::into_value).collect()),
            },
            SendValue::Struct { name, fields } => Value::Struct {
                name: Rc::new(name),
                fields: Rc::new(
                    fields
                        .into_iter()
                        .map(|(k, v)| (k, v.into_value()))
                        .collect(),
                ),
            },
            // ActorPid is left as Nil here — the caller (message dispatch)
            // is responsible for resolving it via the ActorRegistry.
            SendValue::ActorPid(pid) => {
                let _ = pid;
                Value::Nil
            }
        }
    }
}

// --- SendValue equality: structural for all data types ---

impl PartialEq for SendValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SendValue::Int(a), SendValue::Int(b)) => a == b,
            (SendValue::Decimal(a), SendValue::Decimal(b)) => a == b,
            (SendValue::Str(a), SendValue::Str(b)) => a == b,
            (SendValue::Bool(a), SendValue::Bool(b)) => a == b,
            (SendValue::Nil, SendValue::Nil) => true,
            (SendValue::Vec(a), SendValue::Vec(b)) => a == b,
            (SendValue::Map(a), SendValue::Map(b)) => a == b,
            (SendValue::Set(a), SendValue::Set(b)) => a == b,
            (
                SendValue::Variant { name: an, args: aa },
                SendValue::Variant { name: bn, args: ba },
            ) => an == bn && aa == ba,
            (
                SendValue::Struct { name: an, fields: af },
                SendValue::Struct { name: bn, fields: bf },
            ) => an == bn && af == bf,
            (SendValue::ActorPid(a), SendValue::ActorPid(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for SendValue {}

impl Hash for SendValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            SendValue::Int(n) => {
                0u8.hash(state);
                n.hash(state);
            }
            SendValue::Decimal(d) => {
                1u8.hash(state);
                d.hash(state);
            }
            SendValue::Str(s) => {
                2u8.hash(state);
                s.hash(state);
            }
            SendValue::Bool(b) => {
                3u8.hash(state);
                b.hash(state);
            }
            SendValue::Nil => {
                4u8.hash(state);
            }
            SendValue::Vec(v) => {
                5u8.hash(state);
                v.hash(state);
            }
            SendValue::Map(m) => {
                6u8.hash(state);
                m.hash(state);
            }
            SendValue::Set(s) => {
                7u8.hash(state);
                s.hash(state);
            }
            SendValue::Variant { name, args } => {
                9u8.hash(state);
                name.hash(state);
                args.hash(state);
            }
            SendValue::Struct { name, fields } => {
                10u8.hash(state);
                name.hash(state);
                let mut keys: Vec<_> = fields.keys().collect();
                keys.sort();
                for k in keys {
                    k.hash(state);
                    fields[k].hash(state);
                }
            }
            SendValue::ActorPid(pid) => {
                13u8.hash(state);
                pid.hash(state);
            }
        }
    }
}

impl std::fmt::Display for SendValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendValue::Int(n) => write!(f, "{}", n),
            SendValue::Decimal(d) => write!(f, "{}", d),
            SendValue::Str(s) => write!(f, "\"{}\"", s),
            SendValue::Bool(b) => write!(f, "{}", b),
            SendValue::Nil => write!(f, "nil"),
            SendValue::Vec(v) => {
                write!(f, "[")?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            SendValue::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            SendValue::Set(s) => {
                write!(f, "#{{")?;
                for (i, item) in s.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "}}")
            }
            SendValue::Variant { name, args } => {
                if args.is_empty() {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{}(", name)?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", a)?;
                    }
                    write!(f, ")")
                }
            }
            SendValue::Struct { name, fields } => {
                write!(f, "{} {{", name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            SendValue::ActorPid(pid) => write!(f, "{}", pid),
        }
    }
}
