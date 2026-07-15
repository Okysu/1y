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

/// An actor instance: isolated state + message handlers + mailbox.
///
/// Actors run on a single-threaded event loop. `!` enqueues a message and
/// returns immediately; `?` invokes the matching handler synchronously and
/// waits for `reply`. The event loop drains pending `!` messages when the
/// main program finishes (or when explicitly flushed).
pub struct ActorInstance {
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
