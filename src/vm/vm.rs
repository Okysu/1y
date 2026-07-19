//! The VM execution engine.
//!
//! The VM is split into two parts so that multiple actor handlers can run
//! concurrently in their own corosensei coroutines:
//!
//! - [`Vm`] holds **shared global state** (globals, std modules, module
//!   caches, actor definitions, the live-actor registry, and the async
//!   [`Scheduler`]). It is accessed from inside coroutines via a raw pointer
//!   (`unsafe &mut *vm_ptr`), the same pattern the tree-walker's
//!   `Interpreter` uses — safe because the scheduler is single-threaded and
//!   cooperative (only one coroutine runs at a time, and the scheduler never
//!   touches `Vm` fields while a coroutine is resumed).
//!
//! - [`VmCtx`] holds **per-handler execution state** (value stack, call
//!   frames, open upvalues, loop / exception / transact handler stacks). Each
//!   coroutine owns its own `VmCtx`, so they cannot corrupt each other's
//!   frame stacks. The dispatch loop ([`VmCtx::step`]) reads one opcode per
//!   iteration. 1y-level function calls push a frame; no native Rust
//!   recursion is used for them, so recursion depth is bounded by heap, not
//!   the OS stack.
//!
//! Higher-order builtins (`map`/`filter`/`fold`/...) need to call *back* into
//! user closures. That is done via [`VmCtx::call_closure_sync`], which runs a
//! nested dispatch loop until the pushed frame returns. This reintroduces
//! *one* level of native recursion per higher-order call — acceptable because
//! higher-order call nesting is shallow (unlike general recursion such as
//! `fib_memo`, which stays fully on the frame stack).

use crate::ast::Pattern;
use crate::interpreter::error::InterpreterError;
use crate::interpreter::ops;
use crate::value::{ActorEnvVm, ActorRef, ActorInstance, Envelope, ModuleData, NativeFn, SharedRef, Value};
use crate::vm::chunk::{read_u8, read_u16, Chunk, OpCode};
use num_traits::ToPrimitive;
use crate::vm::closure::{ClosureVm, Upvalue, UpvalueRef};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// One call frame. Local variable slot `n` lives at
/// `ctx.stack[frame.stack_base + n]`.
pub struct Frame {
    pub closure: Rc<ClosureVm>,
    pub ip: usize,
    pub stack_base: usize,
    /// Actor state namespace for this frame. `Some` when executing an
    /// actor init chunk, handler body, or actor-local fn. `LoadGlobal` /
    /// `AssignGlobal` / `DefineGlobal` consult this before `globals`.
    pub actor_env: Option<ActorEnvVm>,
}

/// Outcome of a pattern match.
pub enum MatchOutcome {
    /// Matched; carries the bound values in pre-order traversal order.
    Matched(Vec<Value>),
    /// Did not match.
    Failed,
}

/// One transaction's local state: buffered writes + recorded read versions.
/// Keyed by the `SharedRef`'s raw pointer so the same `shared` binding always
/// maps to the same entry.
struct TransactionContext {
    /// `ref_ptr → (SharedRef clone, buffered value)`
    writes: HashMap<usize, (SharedRef, Value)>,
    /// `ref_ptr → (SharedRef clone, version at first read)`
    reads: HashMap<usize, (SharedRef, u64)>,
}

impl TransactionContext {
    fn new() -> Self {
        TransactionContext {
            writes: HashMap::new(),
            reads: HashMap::new(),
        }
    }

    fn ref_id(r: &SharedRef) -> usize {
        Rc::as_ptr(r) as usize
    }
}

/// One entry on the transact handler stack. `retry_addr` is the address of
/// the `PushTransact` instruction (where retries jump back to).
/// `stack_depth` records the stack height at entry, for unwind on exception.
/// `frame_depth` records `frames.len()` at push time so that `retry` only
/// matches handlers pushed within the current frame (a child frame may
/// reuse the same operand-stack range as a parent, so `stack_depth` alone
/// is insufficient).
/// `retry_count` is incremented on each conflict/retry; capped at MAX_RETRIES.
#[derive(Clone)]
struct TransactHandler {
    retry_addr: usize,
    stack_depth: usize,
    frame_depth: usize,
    retry_count: u32,
}

/// Maximum retry attempts for a `transact` block.
const MAX_TXN_RETRIES: u32 = 64;

/// Shared global VM state. Accessed from coroutines via a raw pointer — see
/// the module docs for the safety argument.
pub struct Vm {
    pub globals: HashMap<String, Value>,
    /// Standard library modules (io, env, json, ...). Keyed by dotted path.
    pub std_modules: HashMap<String, crate::value::ModuleRef>,
    /// Cache of already-loaded file modules, keyed by canonical path.
    pub module_cache: HashMap<std::path::PathBuf, crate::value::ModuleRef>,
    /// Stack of modules currently being loaded, for circular-import detection.
    pub module_load_stack: Vec<std::path::PathBuf>,
    /// Entry file's directory; relative imports resolve against this.
    pub entry_dir: Option<std::path::PathBuf>,
    /// Actor definitions: `name → init chunk`. Populated by `DefineActor`.
    /// Used by `Spawn` to instantiate an actor.
    actor_defs: HashMap<String, Rc<Chunk>>,
    /// The actor currently being spawned (set by `Spawn` for the duration
    /// of the init chunk's execution). `RegisterHandler` stores closures
    /// into this actor's `vm_handlers` map.
    current_spawning_actor: Option<ActorRef>,
    /// All live actor instances. Used by `yield` and end-of-program
    /// mailbox draining.
    live_actors: Vec<ActorRef>,
    /// Coroutine-based async scheduler. Drives actor handlers that may
    /// `await` on Tasks. Each spawned handler runs in its own `VmCtx`.
    scheduler: crate::runtime::scheduler::Scheduler,
    /// Persistent type table: `enum` variant name → arity. Shared across
    /// every `compile_program` call (top-level / `import` / `eval`) so
    /// that `eval("Foo.Bar(...)")` recognizes variants defined in the
    /// outer scope. Updated by the compiler whenever an `enum` is
    /// compiled; read by the compiler when a `Name(args)` call is
    /// compiled, to decide between `ConstructVariant` and `Call`.
    variant_table: HashMap<String, usize>,
    /// Persistent type table: `struct` names registered via
    /// `type Name = { ... }`. Same sharing story as `variant_table`.
    struct_table: HashMap<String, ()>,
}

/// One entry on the exception handler stack. `rescue_pc` is the address
/// of the rescue dispatcher (if any); `ensure_pc` is the address of the
/// ensure body (if any). `stack_depth` records the stack height at the
/// time the handler was pushed, so `raise` can truncate back to it.
/// `frame_depth` records `frames.len()` at push time; `handle_signal`
/// matches handlers only within the current frame, since a child frame
/// may reuse the same operand-stack range as its parent.
#[derive(Clone)]
pub struct ExceptionHandler {
    pub rescue_pc: Option<usize>,
    pub ensure_pc: Option<usize>,
    pub stack_depth: usize,
    pub frame_depth: usize,
}

/// One entry on the loop handler stack. Pushed by `PushLoop`, popped by
/// `PopLoop`. `continue_addr` / `break_addr` are jump targets;
/// `stack_depth` is the operand-stack height at push time (for unwind
/// on `break`); `frame_depth` is `frames.len()` at push time so that
/// `Break`/`Continue` only match loops within the current frame.
#[derive(Clone)]
pub struct LoopHandler {
    pub continue_addr: usize,
    pub break_addr: usize,
    pub stack_depth: usize,
    pub frame_depth: usize,
}

/// Per-handler VM execution state. Each coroutine (actor handler) owns its
/// own `VmCtx` so that suspending one handler mid-execution does not corrupt
/// another handler's frame stack.
pub struct VmCtx {
    pub frames: Vec<Frame>,
    pub stack: Vec<Value>,
    /// Open upvalues, sorted by stack index descending (topmost first) so
    /// that closing on frame return is a linear scan from the front.
    pub open_upvalues: Vec<UpvalueRef>,
    /// Loop handler stack. Used to unwind `Break`/`Continue` signals to
    /// the enclosing loop. `frame_depth` records `frames.len()` at push
    /// time so that `Break`/`Continue` only match loops within the
    /// current frame (a child frame may reuse the same operand-stack
    /// range as its parent).
    pub loop_handlers: Vec<LoopHandler>,
    /// Exception handler stack: each `try` pushes one entry. `raise` (or
    /// any `UserException` propagation) unwinds to the nearest handler
    /// whose `stack_depth` is within the current frame.
    pub exception_handlers: Vec<ExceptionHandler>,
    /// The currently-pending exception value, if any. Set by `raise`,
    /// consumed by `EnsureExit` (which either re-raises or clears).
    pub pending_exception: Option<Value>,
    /// Transaction context stack (innermost = last). Non-empty only during
    /// evaluation of a `transact { ... }` body.
    txn_stack: Vec<TransactionContext>,
    /// Transact handler stack: each `PushTransact` pushes one entry. `retry`
    /// or a commit conflict unwinds to the nearest handler. Exceptions also
    /// pop the handler + context when unwinding past it.
    transact_handlers: Vec<TransactHandler>,
}

/// Outcome of [`VmCtx::handle_signal`] when a control-flow signal
/// (`Break` / `Continue` / `Retry` / `UserException` / `Reply` / `Return`)
/// is returned by [`VmCtx::step`].
///
/// Both [`Vm::run_chunk`] and [`VmCtx::run_handler`] route signals through
/// `handle_signal` so that the unwinding rules (loop / transact / exception
/// handler lookup, frame-pop on propagation) stay consistent between the
/// top-level dispatch loop and nested actor-handler dispatch.
enum SignalOutcome {
    /// Signal was consumed internally (e.g. `break` jumped to the loop exit,
    /// `retry` rewound to the transact entry). Continue stepping.
    Continue,
    /// A `Reply(value)` or `Return(value)` signal was caught — the caller
    /// should stop stepping and return `value`.
    Done(Value),
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    pub fn new() -> Self {
        Vm {
            globals: HashMap::new(),
            std_modules: crate::interpreter::stdlib::build_std_modules(),
            module_cache: HashMap::new(),
            module_load_stack: Vec::new(),
            entry_dir: None,
            actor_defs: HashMap::new(),
            current_spawning_actor: None,
            live_actors: Vec::new(),
            scheduler: crate::runtime::scheduler::Scheduler::new(),
            variant_table: HashMap::new(),
            struct_table: HashMap::new(),
        }
    }

    pub fn set_entry_dir(&mut self, dir: std::path::PathBuf) {
        self.entry_dir = Some(dir);
    }

    /// Register the standard builtins (same set the tree-walker uses) into
    /// `globals`, so the VM can call `println`, `get`, `assoc`, ... by name.
    pub fn register_builtins(&mut self) {
        let env = crate::interpreter::env::Environment::global();
        crate::interpreter::builtins::register(&env);
        for (name, val) in env.borrow().bindings_clone() {
            self.globals.insert(name, val);
        }
        // `parallel` is auto-bound as a global (no `import parallel` needed),
        // mirroring the tree-walker.
        if let Some(parallel_mod) = self.std_modules.get("parallel") {
            self.globals
                .insert("parallel".to_string(), Value::Module(parallel_mod.clone()));
        }
    }

    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

    /// Compile and run `source`. Returns the last top-level expression value.
    pub fn run_source(&mut self, source: &str) -> Result<Value, InterpreterError> {
        let output = crate::parser::parse(source);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::RuntimeError {
                msg: format!("parse error: {}", e.full_message()),
                span: Some(e.span),
            });
        }
        let chunk = crate::compiler::compile_program_with_types(
            &output.program,
            &mut self.variant_table,
            &mut self.struct_table,
        )?;
        self.run_chunk(Rc::new(chunk))
    }

    /// Run a top-level chunk to completion.
    pub fn run_chunk(&mut self, chunk: Rc<Chunk>) -> Result<Value, InterpreterError> {
        // The top-level chunk has arity 0; push a frame for it.
        let mut ctx = VmCtx::new();
        let stack_base = ctx.stack.len();
        let closure = Rc::new(ClosureVm::new(chunk, Vec::new()));
        ctx.frames.push(Frame {
            closure,
            ip: 0,
            stack_base,
            actor_env: None,
        });
        // `propagate_depth = 1` keeps the script frame intact: signals that
        // would unwind past it (uncaught exceptions, top-level `return`)
        // escape as `Err`.
        while !ctx.frames.is_empty() {
            match ctx.step(self) {
                Ok(()) => {}
                Err(e) => match ctx.handle_signal(e, 1)? {
                    SignalOutcome::Continue => {}
                    SignalOutcome::Done(_) => {
                        // Reply/Return reached the script frame — treat as
                        // top-level result (mirrors tree-walker).
                        break;
                    }
                },
            }
        }
        // Drain pending `!` messages from live actors, mirroring the
        // tree-walker's `run` behavior. This end-of-program drain is
        // synchronous (fire-and-forget `!` messages); `yield`-driven
        // dispatch uses `drain_mailboxes_async` instead.
        let drain_result = ctx.drain_mailboxes(self);
        // Even if drain errored, return the top-level value (or propagate).
        drain_result?;
        Ok(ctx.stack.pop().unwrap_or(Value::Nil))
    }

    // -----------------------------------------------------------------------
    // Async drain (coroutine-based, mirrors Interpreter::drain_mailboxes_async)
    // -----------------------------------------------------------------------

    /// Like `drain_mailboxes`, but each handler runs in its own coroutine
    /// with its own `VmCtx`, enabling `await` inside handlers. Used by
    /// `OpCode::Yield`.
    ///
    /// # Safety
    /// This method uses a raw pointer to share `self` across coroutines.
    /// It is safe because: (1) single-threaded (Rc/RefCell), (2) corosensei's
    /// `suspend` returns control to this function synchronously and the
    /// scheduler never accesses `self` while a coroutine is resumed, (3)
    /// each coroutine owns its own `VmCtx` so per-handler execution state
    /// is not shared.
    pub fn drain_mailboxes_async(&mut self) -> Result<(), InterpreterError> {
        // Collect all pending messages from all actors.
        let mut pending: Vec<(ActorRef, Value)> = Vec::new();
        for actor_ref in &self.live_actors {
            loop {
                let env_opt = actor_ref.borrow_mut().mailbox.pop_front();
                match env_opt {
                    Some(env) => pending.push((actor_ref.clone(), env.msg)),
                    None => break,
                }
            }
        }

        // Spawn a coroutine for each pending message.
        let self_ptr: *mut Vm = self;
        for (actor_ref, msg) in pending {
            let handler_name = match &msg {
                Value::Variant { name, .. } => name.as_str().to_string(),
                _ => continue, // skip non-variant messages
            };
            let (handler_closure, actor_env) = {
                let inst = actor_ref.borrow();
                let h = match inst.vm_handlers.get(&handler_name) {
                    Some(h) => h.clone(),
                    None => continue, // no handler — skip
                };
                let env = match &inst.vm_env {
                    Some(e) => e.clone(),
                    None => continue, // not a VM actor — skip
                };
                (h, env)
            };
            let args: Vec<Value> = match &msg {
                Value::Variant { args, .. } => args.iter().cloned().collect(),
                _ => unreachable!(),
            };
            if args.len() != handler_closure.arity {
                return Err(InterpreterError::ArityError {
                    expected: handler_closure.arity,
                    got: args.len(),
                    callee: format!("on {}", handler_name),
                    span: Some(crate::ast::Span::dummy()),
                });
            }

            // Spawn coroutine: the closure captures the raw pointer to self.
            // SAFETY: self_ptr is valid for the duration of run_until_complete
            // because `self` outlives the scheduler. The coroutine only
            // accesses the Vm while running (not while suspended), and the
            // scheduler only runs one coroutine at a time.
            self.scheduler.spawn_handler(move || {
                // SAFETY: the coroutine runs inside drain_mailboxes_async,
                // which holds a unique borrow on self. The scheduler never
                // accesses self_ptr while this coroutine is suspended.
                let vm: &mut Vm = unsafe { &mut *self_ptr };
                let mut ctx = VmCtx::new();
                ctx.run_handler(vm, handler_closure, args, actor_env)
            });
        }

        // Advance the scheduler by a bounded number of ticks. Previously
        // this called `run_until_complete`, which blocked the accept loop
        // for the full duration of slow handlers (e.g. `sleep_async(500ms)`),
        // starving incoming connections. With bounded ticks, each `yield`
        // makes progress on in-flight handlers but returns control to the
        // accept loop quickly, so new connections keep being accepted while
        // slow handlers are parked. Any still-pending coroutines remain in
        // the scheduler and resume on the next `yield`.
        //
        // The tick cap is chosen high enough that fast handlers (no await,
        // or sub-ms I/O) finish within a single `yield` call, but low
        // enough that a parked timer doesn't trap us for its full delay.
        const TICKS_PER_YIELD: usize = 64;
        let mut last_results = Vec::new();
        for _ in 0..TICKS_PER_YIELD {
            // Stash a scheduler pointer for register_* calls during this tick.
            // run_until_complete does this internally; tick doesn't, so we
            // set it here. Use a raw pointer dance like run_until_complete.
            let self_sched: *mut crate::runtime::scheduler::Scheduler =
                &mut self.scheduler;
            // SAFETY: self_sched is valid for the duration of this tick.
            // Single-threaded runtime; no concurrent scheduler access.
            unsafe { crate::runtime::scheduler::set_current_scheduler(self_sched) };
            let done = self.scheduler.tick();
            crate::runtime::scheduler::clear_current_scheduler();
            last_results.extend(self.scheduler.take_results());
            if done {
                break;
            }
            // Continue ticking while there's work that can make progress
            // in this `yield` call (see Interpreter::drain_mailboxes_async
            // for the full rationale).
            if !self.scheduler.has_ready() && !self.scheduler.has_pending_timers() {
                break;
            }
        }
        for r in last_results {
            r?;
        }
        Ok(())
    }

    /// `a.b.c` → `<entry_dir>/a/b/c.1y` (or `a/b/c.1y` if no entry_dir).
    fn resolve_module_path(&self, path: &str) -> std::path::PathBuf {
        let relative: std::path::PathBuf = path.split('.').collect();
        let with_ext = relative.with_extension("1y");
        match &self.entry_dir {
            Some(dir) => dir.join(&with_ext),
            None => with_ext,
        }
    }
}

impl VmCtx {
    pub fn new() -> Self {
        VmCtx {
            frames: Vec::new(),
            stack: Vec::new(),
            open_upvalues: Vec::new(),
            loop_handlers: Vec::new(),
            exception_handlers: Vec::new(),
            pending_exception: None,
            txn_stack: Vec::new(),
            transact_handlers: Vec::new(),
        }
    }

    /// Try to consume a control-flow signal returned by [`Self::step`].
    ///
    /// `propagate_depth` is the minimum frame count we may unwind down to
    /// before the signal must escape: `run_chunk` passes `1` (keep the
    /// script frame), `run_handler` / `call_closure_sync` pass the frame
    /// depth at which the nested execution started.
    ///
    /// - `Break` / `Continue` / `Retry` are resolved against the
    ///   nearest loop / transact handler in the current frame; if none
    ///   exists the signal is returned as `Err` (caller surfaces it).
    /// - `UserException` walks the exception-handler stack, popping
    ///   frames down to `propagate_depth` if no handler matches; when it
    ///   can't unwind further the original error is returned as `Err`.
    /// - `Reply` / `Return` unwind to `propagate_depth` and yield
    ///   `SignalOutcome::Done(value)`.
    fn handle_signal(
        &mut self,
        err: InterpreterError,
        propagate_depth: usize,
    ) -> Result<SignalOutcome, InterpreterError> {
        match err {
            InterpreterError::Break { value, .. } => {
                let cur_frame_depth = self.frames.len();
                if let Some(h) = self.loop_handlers.last().cloned() {
                    if h.frame_depth == cur_frame_depth {
                        self.stack.truncate(h.stack_depth);
                        self.stack.push(value.unwrap_or(Value::Nil));
                        self.set_ip(h.break_addr);
                        return Ok(SignalOutcome::Continue);
                    }
                }
                Err(InterpreterError::RuntimeError {
                    msg: "break outside of loop".into(),
                    span: None,
                })
            }
            InterpreterError::Continue { .. } => {
                let cur_frame_depth = self.frames.len();
                if let Some(h) = self.loop_handlers.last().cloned() {
                    if h.frame_depth == cur_frame_depth {
                        self.stack.truncate(h.stack_depth);
                        self.set_ip(h.continue_addr);
                        return Ok(SignalOutcome::Continue);
                    }
                }
                Err(InterpreterError::RuntimeError {
                    msg: "continue outside of loop".into(),
                    span: None,
                })
            }
            InterpreterError::Retry { .. } => {
                let cur_frame_depth = self.frames.len();
                let handler_idx = self
                    .transact_handlers
                    .iter()
                    .rposition(|h| h.frame_depth == cur_frame_depth);
                if let Some(idx) = handler_idx {
                    let handler = self.transact_handlers[idx].clone();
                    let drop_n = self.transact_handlers.len() - idx - 1;
                    for _ in 0..drop_n {
                        self.transact_handlers.pop();
                        self.txn_stack.pop();
                    }
                    self.txn_stack.pop();
                    self.stack.truncate(handler.stack_depth);
                    let new_count = handler.retry_count + 1;
                    if new_count > MAX_TXN_RETRIES {
                        self.transact_handlers.pop();
                        return Err(InterpreterError::RuntimeError {
                            msg: format!("transaction exceeded {} retries", MAX_TXN_RETRIES),
                            span: None,
                        });
                    }
                    self.transact_handlers[idx].retry_count = new_count;
                    self.set_ip(handler.retry_addr);
                    Ok(SignalOutcome::Continue)
                } else {
                    Err(InterpreterError::RuntimeError {
                        msg: "retry outside of transact".into(),
                        span: None,
                    })
                }
            }
            InterpreterError::UserException { value, span } => {
                let mut value = value;
                loop {
                    let cur_frame_depth = self.frames.len();
                    let handler_idx = self
                        .exception_handlers
                        .iter()
                        .rposition(|h| h.frame_depth == cur_frame_depth);
                    if let Some(idx) = handler_idx {
                        let handler = self.exception_handlers[idx].clone();
                        self.exception_handlers.truncate(idx + 1);
                        self.exception_handlers.pop();
                        self.pending_exception = Some(value.clone());
                        self.cleanup_transact_above(handler.stack_depth);
                        self.stack.truncate(handler.stack_depth);
                        self.stack.push(value);
                        if let Some(rescue_pc) = handler.rescue_pc {
                            self.set_ip(rescue_pc);
                        } else if let Some(ensure_pc) = handler.ensure_pc {
                            self.set_ip(ensure_pc);
                        } else {
                            value = self.pending_exception.take().unwrap();
                            continue;
                        }
                        return Ok(SignalOutcome::Continue);
                    } else {
                        if self.frames.len() <= propagate_depth {
                            return Err(InterpreterError::UserException { value, span });
                        }
                        let frame = self.frames.pop().unwrap();
                        self.close_upvalues_above(frame.stack_base);
                        self.cleanup_transact_above(frame.stack_base);
                        self.stack.truncate(frame.stack_base);
                    }
                }
            }
            InterpreterError::Reply { value, .. } => {
                // Unwind all frames down to `propagate_depth`, closing
                // upvalues and rolling back transacts as we go.
                while self.frames.len() > propagate_depth {
                    let frame = self.frames.pop().unwrap();
                    self.close_upvalues_above(frame.stack_base);
                    self.cleanup_transact_above(frame.stack_base);
                }
                self.stack.truncate(self.stack.len());
                Ok(SignalOutcome::Done(value))
            }
            InterpreterError::Return { value, .. } => {
                // `return` pops the frame that emitted it; if that brings
                // us to `propagate_depth`, the caller is the target and
                // the value becomes its result.
                if self.frames.len() > propagate_depth {
                    let frame = self.frames.pop().unwrap();
                    self.close_upvalues_above(frame.stack_base);
                    self.cleanup_transact_above(frame.stack_base);
                    self.stack.truncate(frame.stack_base);
                    self.stack.push(value.clone());
                    Ok(SignalOutcome::Continue)
                } else {
                    Ok(SignalOutcome::Done(value))
                }
            }
            other => Err(other),
        }
    }

    // -----------------------------------------------------------------------
    // Dispatch
    // -----------------------------------------------------------------------

    /// Execute one instruction of the current frame.
    pub fn step(&mut self, vm: &mut Vm) -> Result<(), InterpreterError> {
        let op = self.fetch_op()?;
        match op {
            OpCode::Nil => self.push(Value::Nil),
            OpCode::True => self.push(Value::Bool(true)),
            OpCode::False => self.push(Value::Bool(false)),
            OpCode::Const => {
                let idx = self.read_u8() as usize;
                let v = self.const_at(idx);
                self.push(v);
            }

            // --- locals / upvalues / globals ---
            OpCode::LoadLocal => {
                let slot = self.read_u8() as usize;
                let v = self.local(slot);
                let v = self.resolve_lazy_local(vm, slot, v)?;
                let v = self.read_shared(v);
                self.push(v);
            }
            OpCode::LoadLocalRef => {
                // Like LoadLocal but does NOT auto-deref SharedRef. Used for
                // bare-identifier call args so SharedRef parameters are
                // passed by reference (write-through).
                let slot = self.read_u8() as usize;
                let v = self.local(slot);
                let v = self.resolve_lazy_local(vm, slot, v)?;
                self.push(v);
            }
            OpCode::StoreLocal => {
                // `let` definition: write directly, no write-through.
                let slot = self.read_u8() as usize;
                let v = self.pop();
                self.set_local(slot, v);
            }
            OpCode::LoadUpvalue => {
                let idx = self.read_u8() as usize;
                let f = self.current_closure();
                let uv = f.upvalues[idx].clone();
                let v = uv.read(&self.stack);
                // Lazy import in an upvalue: resolve + rebind in place.
                let v = if let Value::LazyImport { path } = &v {
                    let p = (**path).clone();
                    let m = self.load_module(vm, &p, self.current_span())?;
                    let segments: Vec<&str> = p.split('.').collect();
                    let new_v = if segments.len() > 1 {
                        let root_name = segments[0].to_string();
                        let sub_segments: Vec<&str> = segments[1..].to_vec();
                        Value::Module(build_namespace_module(&sub_segments, m, &root_name))
                    } else {
                        Value::Module(m)
                    };
                    uv.write(&mut self.stack, new_v.clone());
                    new_v
                } else {
                    v
                };
                let v = self.read_shared(v);
                self.push(v);
            }
            OpCode::LoadUpvalueRef => {
                // Like LoadUpvalue but does NOT auto-deref SharedRef.
                let idx = self.read_u8() as usize;
                let f = self.current_closure();
                let uv = f.upvalues[idx].clone();
                let v = uv.read(&self.stack);
                let v = if let Value::LazyImport { path } = &v {
                    let p = (**path).clone();
                    let m = self.load_module(vm, &p, self.current_span())?;
                    let segments: Vec<&str> = p.split('.').collect();
                    let new_v = if segments.len() > 1 {
                        let root_name = segments[0].to_string();
                        let sub_segments: Vec<&str> = segments[1..].to_vec();
                        Value::Module(build_namespace_module(&sub_segments, m, &root_name))
                    } else {
                        Value::Module(m)
                    };
                    uv.write(&mut self.stack, new_v.clone());
                    new_v
                } else {
                    v
                };
                self.push(v);
            }
            OpCode::StoreUpvalue => {
                let idx = self.read_u8() as usize;
                let v = self.pop();
                let f = self.current_closure();
                let uv = f.upvalues[idx].clone();
                uv.write(&mut self.stack, v);
            }
            OpCode::LoadGlobal => {
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                // Actor state takes precedence over module globals when
                // executing inside an actor (handler / init / actor-fn).
                let val = if let Some(env) = &self.frames[self.frames.len() - 1].actor_env {
                    match env.borrow().get(&name) {
                        Some(v) => v.clone(),
                        None => match vm.globals.get(&name) {
                            Some(v) => v.clone(),
                            None => return Err(InterpreterError::NameError {
                                name,
                                span: Some(self.current_span()),
                            }),
                        },
                    }
                } else {
                    match vm.globals.get(&name) {
                        Some(v) => v.clone(),
                        None => return Err(InterpreterError::NameError {
                            name,
                            span: Some(self.current_span()),
                        }),
                    }
                };
                let resolved = if let Value::LazyImport { path } = &val {
                    let p = (**path).clone();
                    let new_v = self.resolve_lazy_import(vm, &p, &name)?;
                    vm.globals.insert(name.clone(), new_v.clone());
                    new_v
                } else {
                    val
                };
                let v = self.read_shared(resolved);
                self.push(v);
            }
            OpCode::LoadGlobalRef => {
                // Like LoadGlobal but does NOT auto-deref SharedRef.
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                let val = if let Some(env) = &self.frames[self.frames.len() - 1].actor_env {
                    match env.borrow().get(&name) {
                        Some(v) => v.clone(),
                        None => match vm.globals.get(&name) {
                            Some(v) => v.clone(),
                            None => return Err(InterpreterError::NameError {
                                name,
                                span: Some(self.current_span()),
                            }),
                        },
                    }
                } else {
                    match vm.globals.get(&name) {
                        Some(v) => v.clone(),
                        None => return Err(InterpreterError::NameError {
                            name,
                            span: Some(self.current_span()),
                        }),
                    }
                };
                let resolved = if let Value::LazyImport { path } = &val {
                    let p = (**path).clone();
                    let new_v = self.resolve_lazy_import(vm, &p, &name)?;
                    vm.globals.insert(name.clone(), new_v.clone());
                    new_v
                } else {
                    val
                };
                self.push(resolved);
            }
            OpCode::StoreGlobal => {
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                let v = self.pop();
                vm.globals.insert(name, v);
            }
            OpCode::DefineGlobal => {
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                let v = self.pop();
                // Inside an actor (init chunk / handler / actor-fn), define
                // in the actor env so state stays actor-local.
                if let Some(env) = &self.frames[self.frames.len() - 1].actor_env {
                    env.borrow_mut().insert(name, v);
                } else {
                    vm.globals.insert(name, v);
                }
            }

            // --- stack ---
            OpCode::Pop => {
                self.pop();
            }
            OpCode::PopN => {
                let n = self.read_u8() as usize;
                self.stack.truncate(self.stack.len() - n);
            }
            OpCode::PopLocalKeep => {
                let n = self.read_u8() as usize;
                let top = self.pop();
                // Close any open upvalues pointing into the region we're
                // about to discard, then truncate.
                let new_len = self.stack.len() - n;
                self.close_upvalues_above(new_len);
                self.stack.truncate(new_len);
                self.push(top);
            }
            OpCode::Dup => {
                let v = self.peek(0).clone();
                self.push(v);
            }

            // --- collection literals ---
            OpCode::NewVec => {
                let n = self.read_u8() as usize;
                let start = self.stack.len() - n;
                let items: Vec<Value> = self.stack.split_off(start);
                self.push(Value::Vec(items.into_iter().collect()));
            }
            OpCode::NewMap => {
                let n = self.read_u8() as usize;
                let start = self.stack.len() - 2 * n;
                let mut map = im::HashMap::new();
                let mut i = start;
                for _ in 0..n {
                    let k = self.stack[i].clone();
                    let v = self.stack[i + 1].clone();
                    map.insert(k, v);
                    i += 2;
                }
                self.stack.truncate(start);
                self.push(Value::Map(map));
            }
            OpCode::NewSet => {
                let n = self.read_u8() as usize;
                let start = self.stack.len() - n;
                let items: Vec<Value> = self.stack.split_off(start);
                let set: im::HashSet<Value> = items.into_iter().collect();
                self.push(Value::Set(set));
            }

            // --- arithmetic / comparison ---
            OpCode::Add => self.binop(ops::add)?,
            OpCode::Sub => self.binop(ops::sub)?,
            OpCode::Mul => self.binop(ops::mul)?,
            OpCode::Div => self.binop(ops::div)?,
            OpCode::Mod => self.binop(ops::modulo)?,
            OpCode::Neg => {
                let a = self.pop();
                self.push(vm_neg(&a)?);
            }
            OpCode::Not => {
                let a = self.pop();
                self.push(Value::Bool(!a.is_truthy()));
            }
            OpCode::Eq => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::Bool(a == b));
            }
            OpCode::Neq => {
                let b = self.pop();
                let a = self.pop();
                self.push(Value::Bool(a != b));
            }
            OpCode::Lt => self.cmp_op(ops::lt)?,
            OpCode::Gt => self.cmp_op(ops::gt)?,
            OpCode::Lte => self.cmp_op(ops::lte)?,
            OpCode::Gte => self.cmp_op(ops::gte)?,

            // --- control flow ---
            OpCode::Jump => {
                let addr = self.read_u16() as usize;
                self.set_ip(addr);
            }
            OpCode::JumpIfFalse => {
                let addr = self.read_u16() as usize;
                let v = self.pop();
                if !v.is_truthy() {
                    self.set_ip(addr);
                }
            }
            OpCode::JumpIfTrue => {
                let addr = self.read_u16() as usize;
                let v = self.pop();
                if v.is_truthy() {
                    self.set_ip(addr);
                }
            }
            OpCode::Loop => {
                let addr = self.read_u16() as usize;
                self.set_ip(addr);
            }

            // --- functions ---
            OpCode::Closure => {
                let chunk_idx = self.read_u8() as usize;
                let closure = self.make_closure(chunk_idx);
                self.push(Value::Closure(closure));
            }
            OpCode::Call => {
                let nargs = self.read_u8() as usize;
                self.do_call(vm, nargs)?;
            }
            OpCode::ConstructVariant => {
                let name_idx = self.read_u8() as usize;
                let nargs = self.read_u8() as usize;
                let name = self.const_name(name_idx);
                let start = self.stack.len() - nargs;
                let args: Vec<Value> = self.stack.split_off(start);
                self.push(Value::Variant {
                    name: Rc::new(name),
                    args: Rc::new(args),
                });
            }
            OpCode::ConstructStruct => {
                // `Name({ field: val, ... })` — pop the Map argument, extract
                // string-keyed entries into a `std::collections::HashMap`,
                // push `Value::Struct`. Mirrors the tree-walker; non-string
                // keys are silently dropped (same as tree-walker).
                let name_idx = self.read_u8() as usize;
                let nargs = self.read_u8() as usize;
                let name = self.const_name(name_idx);
                let span = self.current_span();
                if nargs != 1 {
                    return Err(InterpreterError::ArityError {
                        expected: 1,
                        got: nargs,
                        callee: name,
                        span: Some(span),
                    });
                }
                let map_val = self.pop();
                let mut fields: std::collections::HashMap<String, Value> =
                    std::collections::HashMap::new();
                match &map_val {
                    Value::Map(m) => {
                        for (k, v) in m.iter() {
                            if let Value::Str(s) = k {
                                fields.insert((**s).clone(), v.clone());
                            }
                        }
                    }
                    _ => {
                        return Err(InterpreterError::TypeError {
                            expected: "Map",
                            got: map_val.type_name(),
                            op: format!("constructing {}", name),
                            span: Some(span),
                        });
                    }
                }
                self.push(Value::Struct {
                    name: Rc::new(name),
                    fields: Rc::new(fields),
                });
            }
            OpCode::MethodCall => {
                // Stack: [recv, arg0, ..., arg_{nargs-1}]
                let method_idx = self.read_u8() as usize;
                let nargs = self.read_u8() as usize;
                let method_name = self.const_name(method_idx);
                // Pop args, then recv.
                let args_start = self.stack.len() - nargs;
                let args: Vec<Value> = self.stack.split_off(args_start);
                let recv = self.pop();
                let span = self.current_span();
                match &recv {
                    Value::Module(m) => {
                        let func = m.exports.get(&method_name).cloned().ok_or_else(|| {
                            InterpreterError::ImportError {
                                path: m.name.clone(),
                                msg: format!("no export `{}`", method_name),
                                span: Some(span),
                            }
                        })?;
                        // Call WITHOUT receiver.
                        self.call_value(vm, func, args)?;
                    }
                    _ => {
                        // Look up `method` as a global and call WITH receiver
                        // as the first argument.
                        let func = match vm.globals.get(&method_name) {
                            Some(Value::LazyImport { path }) => {
                                let p = (**path).clone();
                                let new_v = self.resolve_lazy_import(vm, &p, &method_name)?;
                                vm.globals.insert(method_name.clone(), new_v.clone());
                                new_v
                            }
                            Some(v) => v.clone(),
                            None => return Err(InterpreterError::NameError {
                                name: method_name,
                                span: Some(span),
                            }),
                        };
                        let mut full_args = Vec::with_capacity(args.len() + 1);
                        full_args.push(recv);
                        full_args.extend(args);
                        self.call_value(vm, func, full_args)?;
                    }
                }
            }
            OpCode::Return => {
                let ret = self.pop();
                self.return_from_frame(ret)?;
            }

            // --- assignment ---
            OpCode::AssignLocal => {
                let slot = self.read_u8() as usize;
                let v = self.pop();
                self.assign_local(slot, v);
            }
            OpCode::AssignUpvalue => {
                let idx = self.read_u8() as usize;
                let v = self.pop();
                let f = self.current_closure();
                let uv = f.upvalues[idx].clone();
                // Write-through if the upvalue currently holds a SharedRef.
                let cur = uv.read(&self.stack);
                if let Value::Shared(sref) = cur {
                    self.write_shared(sref, v);
                } else {
                    uv.write(&mut self.stack, v);
                }
            }
            OpCode::AssignGlobal => {
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                let v = self.pop();
                // If inside an actor and the name is an actor-state binding,
                // update it there (with SharedRef write-through if applicable).
                let actor_env = self.frames[self.frames.len() - 1].actor_env.clone();
                let handled_in_actor = if let Some(env) = &actor_env {
                    let existing = env.borrow().get(&name).cloned();
                    match existing {
                        Some(Value::Shared(sref)) => {
                            if let Value::Shared(_) = &v {
                                // Replacing the SharedRef itself.
                                let v_clone = v.clone();
                                env.borrow_mut().insert(name.clone(), v_clone);
                            } else {
                                let sref = sref.clone();
                                let v_clone = v.clone();
                                self.write_shared(sref, v_clone);
                            }
                            true
                        }
                        Some(_) => {
                            let v_clone = v.clone();
                            env.borrow_mut().insert(name.clone(), v_clone);
                            true
                        }
                        None => false,
                    }
                } else {
                    false
                };
                if !handled_in_actor {
                    if let Some(Value::Shared(sref)) = vm.globals.get(&name) {
                        let sref = sref.clone();
                        self.write_shared(sref, v);
                    } else {
                        vm.globals.insert(name, v);
                    }
                }
            }

            // --- index / field ---
            OpCode::Index => {
                let idx = self.pop();
                let target = self.pop();
                self.push(self.do_index(&target, &idx)?);
            }
            OpCode::IndexAssign => {
                // Stack: [..., value, coll, idx]
                // Compute new_coll = assoc(coll, idx, value), push new_coll.
                let idx = self.pop();
                let coll = self.pop();
                let value = self.pop();
                let new_coll = ops::assoc(&coll, &idx, &value)?;
                self.push(new_coll);
            }
            OpCode::Field => {
                let idx = self.read_u8() as usize;
                let name = self.const_name(idx);
                let target = self.pop();
                self.push(self.do_field(&target, &name)?);
            }
            OpCode::FieldAssign => {
                let _idx = self.read_u8() as usize;
                let _value = self.pop();
                let _target = self.pop();
                self.push(Value::Nil);
            }

            // --- pattern matching ---
            OpCode::Match => {
                let pat_idx = self.read_u8() as usize;
                let pat = self.pattern_at(pat_idx);
                let value = self.pop();
                match self.vm_match(&pat, &value)? {
                    MatchOutcome::Matched(bindings) => {
                        // Push bindings in order, then true.
                        for b in bindings {
                            self.push(b);
                        }
                        self.push(Value::Bool(true));
                    }
                    MatchOutcome::Failed => {
                        self.push(Value::Bool(false));
                    }
                }
            }

            // --- control-transfer signals (encoded as InterpreterError so
            //     they unwind through arbitrary nested opcodes until caught
            //     by the enclosing loop / function / transaction) ---
            OpCode::Break => {
                let _has_value = self.read_u16();
                let v = self.pop();
                return Err(InterpreterError::Break {
                    value: Some(v),
                    span: Some(self.current_span()),
                });
            }
            OpCode::Continue => {
                let _reserved = self.read_u16();
                return Err(InterpreterError::Continue {
                    span: Some(self.current_span()),
                });
            }
            OpCode::ReturnSignal => {
                let has_value = self.read_u16();
                let v = if has_value != 0 {
                    self.pop()
                } else {
                    Value::Nil
                };
                return Err(InterpreterError::Return {
                    value: v,
                    span: Some(self.current_span()),
                });
            }
            OpCode::Retry => {
                return Err(InterpreterError::Retry {
                    span: Some(self.current_span()),
                });
            }

            // --- exceptions ---
            OpCode::Raise => {
                let v = self.pop();
                // Delegate to the run_chunk-level unwind machinery by
                // returning a UserException. The run_chunk loop will find
                // the nearest handler and jump to it (or propagate out).
                return Err(InterpreterError::UserException {
                    value: v,
                    span: Some(self.current_span()),
                });
            }
            OpCode::PushTry => {
                let rescue_pc = self.read_u16() as usize;
                let ensure_pc = self.read_u16() as usize;
                self.exception_handlers.push(ExceptionHandler {
                    rescue_pc: if rescue_pc == 0 { None } else { Some(rescue_pc) },
                    ensure_pc: if ensure_pc == 0 { None } else { Some(ensure_pc) },
                    stack_depth: self.stack.len(),
                    frame_depth: self.frames.len(),
                });
            }
            OpCode::PopTry => {
                self.exception_handlers.pop();
            }
            OpCode::RescueMatch => {
                let type_idx = self.read_u8() as usize;
                // Stack: [..., exc_value]
                let exc = self.peek(0).clone();
                let matches = if type_idx == 0 {
                    true
                } else {
                    let type_name = self.const_name(type_idx);
                    match &exc {
                        Value::Variant { name, .. } | Value::Struct { name, .. } => {
                            name.as_str() == type_name
                        }
                        _ => false,
                    }
                };
                self.push(Value::Bool(matches));
            }
            OpCode::EnsureExit => {
                // Called at the end of an ensure body. If there is a pending
                // exception, re-raise it (propagate to an outer handler);
                // otherwise fall through (normal exit from try/rescue).
                if let Some(v) = self.pending_exception.take() {
                    return Err(InterpreterError::UserException {
                        value: v,
                        span: Some(self.current_span()),
                    });
                }
                // Else: no-op, continue to the instruction after EnsureExit.
            }
            OpCode::ClearPending => {
                // A rescue clause has successfully matched and is about to
                // run its body. Clear the pending exception so a later
                // `ensure` knows the exception was handled.
                self.pending_exception = None;
            }

            // --- shared ---
            OpCode::SharedExpr => {
                let v = self.pop();
                let cell = crate::value::SharedCell { value: v, version: 0 };
                let sref: SharedRef = Rc::new(RefCell::new(cell));
                self.push(Value::Shared(sref));
            }
            OpCode::PushTransact => {
                let self_addr = self.read_u16() as usize;
                // If the top handler already targets this address, this is a
                // retry: only push a fresh context (handler stays).
                let is_retry = self
                    .transact_handlers
                    .last()
                    .map(|h| h.retry_addr == self_addr)
                    .unwrap_or(false);
                if !is_retry {
                    self.transact_handlers.push(TransactHandler {
                        retry_addr: self_addr,
                        stack_depth: self.stack.len(),
                        frame_depth: self.frames.len(),
                        retry_count: 0,
                    });
                }
                self.txn_stack.push(TransactionContext::new());
            }
            OpCode::TransactCommit => {
                // Pop the transaction context and commit.
                let ctx = self.txn_stack.pop().expect("txn stack imbalance");
                let handler = self
                    .transact_handlers
                    .last()
                    .expect("transact handler missing");
                let is_outermost = self.txn_stack.is_empty();
                if is_outermost {
                    // Validate reads for conflicts.
                    let conflict = ctx
                        .reads
                        .values()
                        .any(|(rref, ver)| Self::shared_committed_version(rref) != *ver);
                    if conflict {
                        // Retry: increment count, check MAX, push fresh ctx, jump.
                        let retry_addr = handler.retry_addr;
                        let new_count = handler.retry_count + 1;
                        if new_count > MAX_TXN_RETRIES {
                            self.transact_handlers.pop();
                            return Err(InterpreterError::RuntimeError {
                                msg: format!(
                                    "transaction exceeded {} retries",
                                    MAX_TXN_RETRIES
                                ),
                                span: Some(self.current_span()),
                            });
                        }
                        self.transact_handlers.last_mut().unwrap().retry_count = new_count;
                        self.txn_stack.push(TransactionContext::new());
                        self.set_ip(retry_addr);
                        // The body's result is still on the stack from before;
                        // pop it since we're restarting.
                        self.pop();
                        return Ok(());
                    }
                    // Apply all writes atomically (bump versions).
                    for (_, (rref, val)) in &ctx.writes {
                        let mut cell = rref.borrow_mut();
                        cell.value = val.clone();
                        cell.version += 1;
                    }
                    // Success: pop handler, fall through with result on stack.
                    self.transact_handlers.pop();
                } else {
                    // Nested commit: merge writes/reads into parent.
                    let parent = self.txn_stack.last_mut().unwrap();
                    for (id, (rref, val)) in ctx.writes {
                        parent.writes.insert(id, (rref, val));
                    }
                    for (id, (rref, ver)) in ctx.reads {
                        parent.reads.entry(id).or_insert((rref, ver));
                    }
                    // Pop the handler (nested transact done).
                    self.transact_handlers.pop();
                }
                // Fall through: result is on stack.
            }

            // --- actor / async ---
            OpCode::DefineActor => {
                let name_idx = self.read_u8() as usize;
                let chunk_idx = self.read_u8() as usize;
                let name = self.const_name(name_idx);
                let init_chunk = {
                    let f = self.current_closure();
                    f.chunk.sub_chunks[chunk_idx].clone()
                };
                vm.actor_defs.insert(name, init_chunk);
            }
            OpCode::RegisterHandler => {
                let name_idx = self.read_u8() as usize;
                let name = self.const_name(name_idx);
                let closure_val = self.pop();
                let handler = match closure_val {
                    Value::Closure(c) => c,
                    other => {
                        return Err(InterpreterError::RuntimeError {
                            msg: format!(
                                "RegisterHandler: expected Closure, got {}",
                                other.type_name()
                            ),
                            span: Some(self.current_span()),
                        });
                    }
                };
                let actor_ref = vm
                    .current_spawning_actor
                    .clone()
                    .ok_or_else(|| InterpreterError::RuntimeError {
                        msg: "RegisterHandler outside of actor init".into(),
                        span: Some(self.current_span()),
                    })?;
                actor_ref
                    .borrow_mut()
                    .vm_handlers
                    .insert(name, handler);
            }
            OpCode::Spawn => {
                let name_idx = self.read_u8() as usize;
                let nargs = self.read_u8() as usize;
                let name = self.const_name(name_idx);
                let init_chunk = vm
                    .actor_defs
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| InterpreterError::RuntimeError {
                        msg: format!("actor `{}` is not defined", name),
                        span: Some(self.current_span()),
                    })?;
                // Pop constructor args.
                let args_start = self.stack.len() - nargs;
                let args: Vec<Value> = self.stack.split_off(args_start);
                // (1y actor bodies have arity 0 for now; we ignore the
                // args — but check the count is 0 to mirror the tree-walker,
                // which also has no constructor params.)
                if !args.is_empty() {
                    return Err(InterpreterError::RuntimeError {
                        msg: format!(
                            "actor `{}` takes no constructor arguments, got {}",
                            name,
                            args.len()
                        ),
                        span: Some(self.current_span()),
                    });
                }
                self.do_spawn(vm, name, init_chunk)?;
            }
            OpCode::ActorSend => {
                // Stack: [actor, msg]
                let msg = self.pop();
                let actor_val = self.pop();
                let actor_ref = match &actor_val {
                    Value::Actor(ar) => ar.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Actor",
                            got: other.type_name(),
                            op: "send (`!`)".into(),
                            span: Some(self.current_span()),
                        });
                    }
                };
                // The message will be dispatched in a NEW coroutine (via
                // drain_mailboxes_async) with its own VmCtx/stack. Any VM
                // closures in the message have OPEN upvalues pointing into
                // THIS ctx's stack; they would read garbage in the receiver's
                // ctx. Close them now so the captured values are moved to
                // the heap (Closed state).
                self.close_escaping_upvalues(&msg);
                actor_ref.borrow_mut().mailbox.push_back(Envelope {
                    msg,
                    reply_slot: None,
                });
                self.push(Value::Nil);
            }
            OpCode::ActorRequest => {
                // Stack: [actor, msg]
                let msg = self.pop();
                let actor_val = self.pop();
                let span = self.current_span();
                let actor_ref = match &actor_val {
                    Value::Actor(ar) => ar.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Actor",
                            got: other.type_name(),
                            op: "request (`?`)".into(),
                            span: Some(span),
                        });
                    }
                };
                let reply = self.dispatch_request(vm, &actor_ref, &msg, span)?;
                self.push(reply);
            }
            OpCode::Yield => {
                // Drain all live actors' mailboxes via coroutines so that
                // handlers may `await`. Mirrors the tree-walker's Yield.
                vm.drain_mailboxes_async()?;
                self.push(Value::Nil);
            }
            OpCode::Reply => {
                let has_value = self.read_u16() != 0;
                let value = if has_value { self.pop() } else { Value::Nil };
                return Err(InterpreterError::Reply {
                    value,
                    span: Some(self.current_span()),
                });
            }
            OpCode::Await => {
                // Zig-style colorless async: pop a Task, then either suspend
                // the current coroutine (if inside one) or busy-wait poll
                // (top-level fallback). Mirrors the tree-walker's Await.
                let task_val = self.pop();
                let task_ref = match &task_val {
                    Value::Task(t) => t.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Task",
                            got: other.type_name(),
                            op: "await".into(),
                            span: Some(self.current_span()),
                        });
                    }
                };
                if crate::runtime::scheduler::in_coroutine() {
                    // Inside a coroutine: suspend and let the scheduler poll
                    // I/O / timers and resume us when the task is ready.
                    let v = crate::runtime::scheduler::await_task(task_ref);
                    self.push(v);
                } else {
                    // Top-level fallback: poll synchronously, but drive the
                    // scheduler between polls so parked coroutines (e.g.
                    // slow handlers on `sleep_async`) make progress while
                    // the top-level await waits (e.g. accept loop on
                    // `accept_async`). Without this, a top-level await
                    // would busy-wait and starve all parked coroutines.
                    loop {
                        // Drive the scheduler: advance parked coroutines by
                        // a bounded number of ticks. This lets slow handlers
                        // resume and complete while we wait for our own Task.
                        vm.drain_mailboxes_async()?;
                        let ready = {
                            let t = task_ref.borrow();
                            match &*t {
                                crate::value::TaskState::Ready(v) => Some(v.clone()),
                                crate::value::TaskState::Consumed => Some(Value::Nil),
                                crate::value::TaskState::Pending(f, _) => match f() {
                                    crate::value::TaskPoll::Ready(v) => Some(v),
                                    crate::value::TaskPoll::Pending => None,
                                },
                            }
                        };
                        if let Some(v) = ready {
                            *task_ref.borrow_mut() = crate::value::TaskState::Consumed;
                            self.push(v);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }

            // --- module system ---
            OpCode::Import => {
                let path_idx = self.read_u8() as usize;
                let alias_idx = self.read_u8() as usize;
                let lazy = self.read_u8() != 0;
                let path = match self.const_at(path_idx) {
                    Value::Str(s) => (*s).clone(),
                    _ => return Err(InterpreterError::RuntimeError {
                        msg: "Import: path const must be a string".into(),
                        span: Some(self.current_span()),
                    }),
                };
                let alias = if alias_idx == 0 {
                    None
                } else {
                    match self.const_at(alias_idx) {
                        Value::Str(s) => Some((*s).clone()),
                        _ => None,
                    }
                };

                if let Some(alias_name) = alias {
                    // With alias: flat binding (alias → module), regardless of
                    // whether the path is dotted.
                    if lazy {
                        let val = Value::LazyImport { path: Rc::new(path) };
                        self.bind_current(vm, alias_name, val);
                    } else {
                        let m = self.load_module(vm, &path, self.current_span())?;
                        self.bind_current(vm, alias_name, Value::Module(m));
                    }
                } else {
                    let segments: Vec<&str> = path.split('.').collect();
                    if segments.len() == 1 {
                        // Simple name: bind directly.
                        if lazy {
                            let val = Value::LazyImport { path: Rc::new(path.clone()) };
                            self.bind_current(vm, segments[0].to_string(), val);
                        } else {
                            let m = self.load_module(vm, &path, self.current_span())?;
                            self.bind_current(vm, segments[0].to_string(), Value::Module(m));
                        }
                    } else {
                        // Dotted path: bind the root as a namespace module
                        // containing the sub-path. On lazy import, defer: bind
                        // the root as a LazyImport; on first access, build the
                        // namespace tree (see resolve_lazy_*).
                        let root_name = segments[0].to_string();
                        if lazy {
                            let val = Value::LazyImport { path: Rc::new(path.clone()) };
                            self.bind_current(vm, root_name, val);
                        } else {
                            let m = self.load_module(vm, &path, self.current_span())?;
                            self.bind_dotted_namespace(vm, &path, m);
                        }
                    }
                }
            }

            // --- stack manipulation ---
            OpCode::Swap => {
                let len = self.stack.len();
                self.stack.swap(len - 1, len - 2);
            }
            OpCode::Rot3 => {
                // [a, b, c] → [b, c, a]
                let len = self.stack.len();
                let a = self.stack[len - 3].clone();
                let b = self.stack[len - 2].clone();
                let c = self.stack[len - 1].clone();
                self.stack[len - 3] = b;
                self.stack[len - 2] = c;
                self.stack[len - 1] = a;
            }

            // --- loop handlers ---
            OpCode::PushLoop => {
                let continue_addr = self.read_u16() as usize;
                let break_addr = self.read_u16() as usize;
                self.loop_handlers.push(LoopHandler {
                    continue_addr,
                    break_addr,
                    stack_depth: self.stack.len(),
                    frame_depth: self.frames.len(),
                });
            }
            OpCode::PopLoop => {
                self.loop_handlers.pop();
            }

            OpCode::_End => {
                return Err(InterpreterError::RuntimeError {
                    msg: "hit end-of-opcode sentinel".into(),
                    span: Some(self.current_span()),
                });
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Call machinery
    // -----------------------------------------------------------------------

    fn do_call(&mut self, vm: &mut Vm, nargs: usize) -> Result<(), InterpreterError> {
        // Stack layout: [..., callee, arg0, ..., arg_{nargs-1}]
        let callee_idx = self.stack.len() - 1 - nargs;
        let callee = self.stack[callee_idx].clone();
        let args: Vec<Value> = self.stack.split_off(callee_idx + 1);
        self.stack.pop(); // remove callee
        self.call_value(vm, callee, args)
    }

    fn call_value(
        &mut self,
        vm: &mut Vm,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<(), InterpreterError> {
        match callee {
            Value::Closure(c) => {
                if args.len() != c.arity {
                    return Err(InterpreterError::ArityError {
                        expected: c.arity,
                        got: args.len(),
                        callee: c.name.clone().unwrap_or_else(|| "<lambda>".into()),
                        span: Some(self.current_span()),
                    });
                }
                let actor_env = c.actor_env.clone();
                let stack_base = self.stack.len();
                self.stack.extend(args);
                self.frames.push(Frame {
                    closure: c,
                    ip: 0,
                    stack_base,
                    actor_env,
                });
                Ok(())
            }
            Value::Native(nf) => self.call_native(vm, nf, args),
            Value::Func(_) => Err(InterpreterError::RuntimeError {
                msg: "tree-walker closure cannot be called from the VM".into(),
                span: Some(self.current_span()),
            }),
            other => Err(InterpreterError::TypeError {
                expected: "callable",
                got: other.type_name(),
                op: "call".into(),
                span: Some(self.current_span()),
            }),
        }
    }

    fn call_native(
        &mut self,
        vm: &mut Vm,
        nf: Rc<NativeFn>,
        args: Vec<Value>,
    ) -> Result<(), InterpreterError> {
        // Higher-order builtins route to VM methods so they can call user
        // closures. Everything else is a plain native call.
        let span = self.current_span();
        match nf.name {
            "map" => self.ho_map(vm, &args, span),
            "filter" => self.ho_filter(vm, &args, span),
            "fold" => self.ho_fold(vm, &args, span),
            "reduce" => self.ho_reduce(vm, &args, span),
            "find" => self.ho_find(vm, &args, span),
            "each" => self.ho_each(vm, &args, span),
            "eval" => {
                let v = self.eval_src(vm, &args, span)?;
                self.push(v);
                Ok(())
            }
            _ => {
                // Auto-deref SharedRef arguments, matching the tree-walker.
                let args: Vec<Value> = args
                    .into_iter()
                    .map(|a| match &a {
                        Value::Shared(sref) => sref.borrow().value.clone(),
                        _ => a,
                    })
                    .collect();
                let v = (nf.func)(&args).map_err(|e| e.with_span(span))?;
                self.push(v);
                Ok(())
            }
        }
    }

    /// Call a user closure synchronously and return its result. Used by the
    /// higher-order builtins. Runs a nested dispatch loop until the pushed
    /// frame returns.
    fn call_closure_sync(
        &mut self,
        vm: &mut Vm,
        closure: Rc<ClosureVm>,
        args: Vec<Value>,
    ) -> Result<Value, InterpreterError> {
        if args.len() != closure.arity {
            return Err(InterpreterError::ArityError {
                expected: closure.arity,
                got: args.len(),
                callee: closure.name.clone().unwrap_or_else(|| "<lambda>".into()),
                span: Some(self.current_span()),
            });
        }
        let stack_base = self.stack.len();
        self.stack.extend(args);
        let target_depth = self.frames.len();
        let saved_loop_len = self.loop_handlers.len();
        let actor_env = closure.actor_env.clone();
        self.frames.push(Frame {
            closure,
            ip: 0,
            stack_base,
            actor_env,
        });
        // `handle_signal(target_depth)` routes Break/Continue/Retry to
        // handlers within the closure's own frame; if none exists the
        // signal escapes as an `Err` (mirrors the tree-walker, where these
        // signals don't cross function boundaries). Reply/Return unwind
        // to `target_depth` and yield `Done(value)`.
        let result: Result<Value, InterpreterError> = loop {
            if self.frames.len() <= target_depth {
                break Ok(self.stack.pop().unwrap_or(Value::Nil));
            }
            match self.step(vm) {
                Ok(()) => {}
                Err(e) => match self.handle_signal(e, target_depth) {
                    Ok(SignalOutcome::Continue) => {}
                    Ok(SignalOutcome::Done(v)) => break Ok(v),
                    Err(err) => break Err(err),
                },
            }
        };
        self.loop_handlers.truncate(saved_loop_len);
        while self.frames.len() > target_depth {
            let f = self.frames.pop().unwrap();
            self.close_upvalues_above(f.stack_base);
        }
        self.stack.truncate(stack_base);
        result
    }

    fn return_from_frame(&mut self, ret: Value) -> Result<(), InterpreterError> {
        let frame = self.frames.pop().expect("frame to return from");
        // Close any open upvalues pointing into this frame's stack region.
        self.close_upvalues_above(frame.stack_base);
        // Drop any loop/exception/transact handlers that belonged to this
        // frame (or any inner frame). After popping `frame`, `frames.len()`
        // is the depth of the caller; any handler whose `frame_depth` is
        // greater than that belongs to a frame we just left, so it must be
        // removed — otherwise a later `break`/`continue`/`raise` in the
        // caller could match a stale handler and mis-dispatch.
        let caller_depth = self.frames.len();
        self.loop_handlers.retain(|h| h.frame_depth <= caller_depth);
        self.exception_handlers.retain(|h| h.frame_depth <= caller_depth);
        self.transact_handlers.retain(|h| h.frame_depth <= caller_depth);
        // Truncate the stack back to the frame's base, then push the return value.
        self.stack.truncate(frame.stack_base);
        self.stack.push(ret);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Actor system
    // -----------------------------------------------------------------------

    /// Instantiate actor `name` by running its init chunk. The init chunk's
    /// `state` decls / `fn` defs populate the actor's `vm_env`, and its
    /// `on` clauses register handler closures into `vm_handlers` (via
    /// `RegisterHandler` which reads `current_spawning_actor`).
    /// After init, binds `self = Value::Actor(actor_ref)` in the actor env,
    /// records the actor in `live_actors`, and pushes `Value::Actor` onto
    /// the stack.
    fn do_spawn(
        &mut self,
        vm: &mut Vm,
        _name: String,
        init_chunk: Rc<Chunk>,
    ) -> Result<(), InterpreterError> {
        let actor_env: ActorEnvVm = Rc::new(RefCell::new(HashMap::new()));
        // Build a placeholder ActorInstance with the env wired up. We'll
        // run the init chunk with `current_spawning_actor` set so
        // RegisterHandler can populate `vm_handlers`.
        let actor_ref: ActorRef = {
            let mut inst = ActorInstance::new(
                crate::interpreter::env::Environment::global(),
            );
            inst.vm_env = Some(actor_env.clone());
            Rc::new(RefCell::new(inst))
        };
        // Save & set current_spawning_actor.
        let prev_spawning = vm.current_spawning_actor.take();
        vm.current_spawning_actor = Some(actor_ref.clone());

        // Run the init chunk in a new frame with `actor_env` set. The init
        // chunk has arity 0.
        let closure = Rc::new(ClosureVm::new(init_chunk, Vec::new()));
        let stack_base = self.stack.len();
        let target_depth = self.frames.len();
        let saved_loop_len = self.loop_handlers.len();
        self.frames.push(Frame {
            closure: closure.clone(),
            ip: 0,
            stack_base,
            actor_env: Some(actor_env.clone()),
        });
        // Run until the init frame returns. Catch errors to ensure
        // current_spawning_actor is restored.
        let init_result: Result<(), InterpreterError> = (|| {
            while self.frames.len() > target_depth {
                match self.step(vm) {
                    Ok(()) => {}
                    Err(InterpreterError::Break { .. })
                    | Err(InterpreterError::Continue { .. }) => {
                        self.loop_handlers.truncate(saved_loop_len);
                        return Err(InterpreterError::RuntimeError {
                            msg: "break/continue escaping actor init".into(),
                            span: Some(self.current_span()),
                        });
                    }
                    Err(e) => {
                        self.loop_handlers.truncate(saved_loop_len);
                        return Err(e);
                    }
                }
            }
            // Init chunk returned; pop its return value (Nil).
            let _ = self.stack.pop();
            Ok(())
        })();

        // Restore current_spawning_actor regardless of success/failure.
        vm.current_spawning_actor = prev_spawning;
        init_result?;

        // Bind `self` in the actor env so handlers can reference it.
        actor_env
            .borrow_mut()
            .insert("self".to_string(), Value::Actor(actor_ref.clone()));
        // Track in live_actors for `yield` / end-of-program drain.
        vm.live_actors.push(actor_ref.clone());
        // Push the Actor value as the spawn expression's result.
        self.push(Value::Actor(actor_ref));
        Ok(())
    }

    /// Synchronously dispatch `msg` to the actor's matching handler.
    /// Returns the reply value (or Nil if the handler fell through without
    /// `reply`).
    fn dispatch_request(
        &mut self,
        vm: &mut Vm,
        actor_ref: &ActorRef,
        msg: &Value,
        span: crate::ast::Span,
    ) -> Result<Value, InterpreterError> {
        let handler_name = match msg {
            Value::Variant { name, .. } => name.as_str().to_string(),
            other => {
                return Err(InterpreterError::RuntimeError {
                    msg: format!(
                        "actor messages must be variants, got {}",
                        other.type_name()
                    ),
                    span: Some(span),
                });
            }
        };
        let (handler_closure, actor_env) = {
            let inst = actor_ref.borrow();
            let h = inst
                .vm_handlers
                .get(&handler_name)
                .cloned()
                .ok_or_else(|| InterpreterError::RuntimeError {
                    msg: format!(
                        "actor has no `on {}` handler for message `{}`",
                        handler_name, handler_name
                    ),
                    span: Some(span),
                })?;
            let env = inst
                .vm_env
                .clone()
                .ok_or_else(|| InterpreterError::RuntimeError {
                    msg: "actor was not created by the VM".into(),
                    span: Some(span),
                })?;
            (h, env)
        };
        // Arity check: handler params count must match message args count.
        let args: Vec<Value> = match msg {
            Value::Variant { args, .. } => args.iter().cloned().collect(),
            _ => unreachable!(),
        };
        if args.len() != handler_closure.arity {
            return Err(InterpreterError::ArityError {
                expected: handler_closure.arity,
                got: args.len(),
                callee: format!("on {}", handler_name),
                span: Some(span),
            });
        }
        self.run_handler(vm, handler_closure, args, actor_env)
    }

    /// Push a handler frame and run `step` until it returns. Catches
    /// `Reply` as the reply value; any other error propagates. Used by both
    /// `dispatch_request` (synchronous `?`) and `drain_mailboxes_async`
    /// (each coroutine owns its own `VmCtx`).
    fn run_handler(
        &mut self,
        vm: &mut Vm,
        handler_closure: Rc<ClosureVm>,
        args: Vec<Value>,
        actor_env: ActorEnvVm,
    ) -> Result<Value, InterpreterError> {
        let stack_base = self.stack.len();
        self.stack.extend(args);
        let target_depth = self.frames.len();
        let saved_loop_len = self.loop_handlers.len();
        self.frames.push(Frame {
            closure: handler_closure,
            ip: 0,
            stack_base,
            actor_env: Some(actor_env),
        });
        let result: Result<Value, InterpreterError> = loop {
            if self.frames.len() <= target_depth {
                // Handler frame returned normally — discard the body value
                // (mirrors the tree-walker, which returns Nil when a handler
                // falls through without `reply`).
                let _ = self.stack.pop();
                break Ok(Value::Nil);
            }
            match self.step(vm) {
                Ok(()) => {}
                Err(e) => match self.handle_signal(e, target_depth) {
                    Ok(SignalOutcome::Continue) => {}
                    Ok(SignalOutcome::Done(v)) => break Ok(v),
                    Err(err) => break Err(err),
                },
            }
        };
        // Always restore loop-handler stack and truncate handler-local
        // stack so the caller sees a clean state whether we succeeded or
        // errored. The result value (if any) is re-pushed after truncation.
        self.loop_handlers.truncate(saved_loop_len);
        // Pop any frames still above target_depth (e.g. on error escape).
        while self.frames.len() > target_depth {
            let f = self.frames.pop().unwrap();
            self.close_upvalues_above(f.stack_base);
        }
        self.stack.truncate(stack_base);
        result
    }

    /// Process all pending `!` messages in every live actor's mailbox.
    /// Called at end of `run_chunk`. Synchronous (no coroutines).
    fn drain_mailboxes(&mut self, vm: &mut Vm) -> Result<(), InterpreterError> {
        // Iterate by index so newly-spawned actors get processed too.
        let mut i = 0;
        while i < vm.live_actors.len() {
            let actor_ref = vm.live_actors[i].clone();
            loop {
                let env = {
                    let mut inst = actor_ref.borrow_mut();
                    match inst.mailbox.pop_front() {
                        Some(e) => e,
                        None => break,
                    }
                };
                // Dispatch the message synchronously (ignoring reply).
                // Errors during drain are propagated.
                let _ = self.dispatch_request(vm, &actor_ref, &env.msg, crate::ast::Span::dummy())?;
            }
            i += 1;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Module system
    // -----------------------------------------------------------------------

    /// Bind `name = value` in the current scope: at top level (script frame)
    /// this writes to `globals`; inside a function it writes to a local slot
    /// reserved by the compiler. For phase-A VM we always emit Import at the
    /// top level, so a global store suffices; but we keep this generic for
    /// future nested imports.
    fn bind_current(&mut self, vm: &mut Vm, name: String, val: Value) {
        // Heuristic: if the current frame is the script (top-level) chunk,
        // store as a global. Otherwise store as a fresh local at the end of
        // the frame. The script chunk's name is `Some("<script>")`, set by
        // `Compiler::new(FunctionType::Script)`.
        let is_script = self
            .frames
            .last()
            .map(|f| match &f.closure.name {
                Some(n) => n == "<script>",
                None => true,
            })
            .unwrap_or(true);
        if is_script {
            vm.globals.insert(name, val);
        } else {
            // Append to the current frame's local region.
            self.stack.push(val);
            // (We do not track this slot in the compiler's local table; future
            // references must go through LoadGlobal/StoreGlobal. This branch
            // is only a safety fallback for nested imports.)
        }
    }

    // --- dotted-path namespace binding ---
    //
    // `import a.b.c` (no alias) binds `a` as a namespace Module whose exports
    // contain `b` → { `c` → leaf_module }. If `a` already exists as a Module
    // (from a prior `import a.x`), the new sub-path is merged in.

    /// Bind a dotted-path module as a namespace tree rooted at `segments[0]`.
    /// Merges with an existing root Module if present.
    fn bind_dotted_namespace(
        &mut self,
        vm: &mut Vm,
        path: &str,
        leaf: crate::value::ModuleRef,
    ) {
        let segments: Vec<&str> = path.split('.').collect();
        let root_name = segments[0].to_string();
        let sub_segments: Vec<&str> = segments[1..].to_vec();

        let existing = vm.globals.get(&root_name).cloned();
        let root_module = match existing {
            Some(Value::Module(m)) => merge_namespace(&m, &sub_segments, leaf),
            _ => build_namespace_module(&sub_segments, leaf, &root_name),
        };
        self.bind_current(vm, root_name, Value::Module(root_module));
    }

    /// Resolve a LazyImport value into a Module. For dotted paths without an
    /// alias (bind_name == first segment), builds (or merges into) a namespace
    /// tree. For alias bindings, returns the leaf module directly.
    fn resolve_lazy_import(
        &mut self,
        vm: &mut Vm,
        path: &str,
        bind_name: &str,
    ) -> Result<Value, InterpreterError> {
        let m = self.load_module(vm, path, self.current_span())?;
        let segments: Vec<&str> = path.split('.').collect();
        let new_v = if segments.len() > 1 && bind_name == segments[0] {
            // No-alias dotted import: bind root as namespace.
            let root_name = segments[0].to_string();
            let sub_segments: Vec<&str> = segments[1..].to_vec();
            let existing = vm.globals.get(&root_name).cloned();
            match existing {
                Some(Value::Module(em)) => {
                    Value::Module(merge_namespace(&em, &sub_segments, m))
                }
                _ => {
                    Value::Module(build_namespace_module(&sub_segments, m, &root_name))
                }
            }
        } else {
            // Alias or simple-name import: bind leaf module directly.
            Value::Module(m)
        };
        Ok(new_v)
    }

    /// If the value at local `slot` is a `LazyImport`, load it and write the
    /// resolved `Module` back into the slot. Returns the resolved value.
    /// For dotted paths, builds a namespace tree (no merging — locals are
    /// scope-isolated).
    fn resolve_lazy_local(
        &mut self,
        vm: &mut Vm,
        slot: usize,
        val: Value,
    ) -> Result<Value, InterpreterError> {
        if let Value::LazyImport { path } = &val {
            let p = (**path).clone();
            let m = self.load_module(vm, &p, self.current_span())?;
            let segments: Vec<&str> = p.split('.').collect();
            let new_v = if segments.len() > 1 {
                let root_name = segments[0].to_string();
                let sub_segments: Vec<&str> = segments[1..].to_vec();
                Value::Module(build_namespace_module(&sub_segments, m, &root_name))
            } else {
                Value::Module(m)
            };
            self.set_local(slot, new_v.clone());
            Ok(new_v)
        } else {
            Ok(val)
        }
    }

    /// Load a module by dotted path. Stdlib first, then file system. File
    /// modules are evaluated in a fresh child frame on the current `VmCtx`
    /// (sharing globals + module caches via `vm`), and their top-level
    /// bindings become the module's exports.
    pub fn load_module(
        &mut self,
        vm: &mut Vm,
        path: &str,
        span: crate::ast::Span,
    ) -> Result<crate::value::ModuleRef, InterpreterError> {
        // 1. stdlib?
        if let Some(m) = vm.std_modules.get(path) {
            return Ok(m.clone());
        }

        // 2. resolve file path
        let file_path = vm.resolve_module_path(path);
        let canonical = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return Err(InterpreterError::ImportError {
                    path: path.to_string(),
                    msg: format!("module file not found: {}", file_path.display()),
                    span: Some(span),
                });
            }
        };

        // 3. cache
        if let Some(m) = vm.module_cache.get(&canonical) {
            return Ok(m.clone());
        }

        // 4. circular import?
        if vm.module_load_stack.iter().any(|p| p == &canonical) {
            let chain: Vec<String> = vm
                .module_load_stack
                .iter()
                .map(|p| p.display().to_string())
                .chain(std::iter::once(canonical.display().to_string()))
                .collect();
            return Err(InterpreterError::ImportError {
                path: path.to_string(),
                msg: format!("circular import: {}", chain.join(" -> ")),
                span: Some(span),
            });
        }

        // 5. read + parse
        let source = std::fs::read_to_string(&canonical).map_err(|e| {
            InterpreterError::ImportError {
                path: path.to_string(),
                msg: format!("failed to read {}: {}", canonical.display(), e),
                span: Some(span),
            }
        })?;
        let output = crate::parser::parse(&source);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::ImportError {
                path: path.to_string(),
                msg: format!("parse error: {}", e.full_message()),
                span: Some(span),
            });
        }

        // 6. compile to a top-level chunk (share VM's persistent type
        //    table so module-level `enum`/`type` definitions are visible
        //    to later `eval` / `import`).
        let chunk = crate::compiler::compile_program_with_types(
            &output.program,
            &mut vm.variant_table,
            &mut vm.struct_table,
        )?;

        // 7. run it on a child frame, capturing top-level globals as exports.
        //    We snapshot globals before, run, then diff to extract exports.
        let snapshot: HashMap<String, Value> = vm.globals.clone();
        vm.module_load_stack.push(canonical.clone());
        let prev_len = self.stack.len();
        let frame_count = self.frames.len();
        let closure = Rc::new(ClosureVm::new(Rc::new(chunk), Vec::new()));
        self.frames.push(Frame {
            closure,
            ip: 0,
            stack_base: prev_len,
            actor_env: None,
        });
        // Run only the module's frame — stop when it returns. Do NOT keep
        // stepping while `frames` is non-empty, because the calling frame
        // (script or another module) is still on the stack.
        //
        // Control-flow signals (Break/Continue/Retry/UserException/Return/Reply)
        // produced inside the module must be handled HERE, with
        // `propagate_depth = frame_count`, so that e.g. a `break` inside the
        // module's `loop { ... }` is caught by the module's own loop handler
        // instead of escaping up to the caller's `handle_signal` (which could
        // find a stale loop handler from the module and "continue" execution
        // in the module frame after load_module has already returned Err).
        while self.frames.len() > frame_count {
            match self.step(vm) {
                Ok(()) => {}
                Err(e) => match self.handle_signal(e, frame_count)? {
                    SignalOutcome::Continue => {}
                    SignalOutcome::Done(_) => {
                        // Return/Reply reached the module's script frame —
                        // treat as module completion (mirrors run_chunk).
                        break;
                    }
                },
            }
        }
        // pop the nil/last-expr value left by the module's Return
        let _ = self.stack.pop();
        vm.module_load_stack.pop();

        // 8. collect exports = globals introduced or *overwritten* by the
        //    module. We can't just diff keys, because a module may legitimately
        //    rebind a name that also exists as a builtin (e.g. lib/yin.1y
        //    defines `fn get(...)`, shadowing the builtin `get`). If the
        //    value differs from the snapshot, the module overwrote it, so
        //    expose the new value as an export.
        let mut exports: HashMap<String, Value> = HashMap::new();
        for (k, v) in &vm.globals {
            match snapshot.get(k) {
                None => exports.insert(k.clone(), v.clone()),
                Some(old) if old != v => exports.insert(k.clone(), v.clone()),
                _ => None,
            };
        }

        // 9. Restore globals that the module overwrote (e.g. a module
        //    defining `fn get(...)` would shadow the builtin `get` used by
        //    other modules like lib/http.1y). New globals introduced by the
        //    module (its own functions, nested `import ... as name` bindings)
        //    are kept so the module's closures can still resolve them.
        //    Exports were already collected above, so `module.get` still
        //    points to the module's `get` even after we restore the builtin.
        for (k, old_v) in &snapshot {
            if let Some(new_v) = vm.globals.get(k) {
                if new_v != old_v {
                    vm.globals.insert(k.clone(), old_v.clone());
                }
            }
        }

        let module = Rc::new(crate::value::ModuleData {
            name: path.to_string(),
            source_path: Some(canonical.clone()),
            exports,
        });
        vm.module_cache.insert(canonical, module.clone());
        Ok(module)
    }

    // -----------------------------------------------------------------------
    // Dynamic eval
    // -----------------------------------------------------------------------

    /// `eval(src)` — parse, compile, and execute a source string on the
    /// current `VmCtx`, sharing `vm.globals` so definitions persist.
    /// Returns the value of the last top-level expression (or `Nil`).
    ///
    /// Mirrors `Interpreter::eval_src` in the tree-walker. The compiled
    /// chunk runs on a child frame pushed onto the current `VmCtx`; we step
    /// until that frame returns, then pop the result. Control signals
    /// (`Break`/`Continue`/`Retry`/`UserException`/`Reply`/`Return`) are
    /// routed through `handle_signal` with `target_depth` set to the
    /// pre-eval frame count, so an uncaught `raise` inside `eval` propagates
    /// out as `Err` (matching the tree-walker) and a top-level `return`
    /// yields its value.
    fn eval_src(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<Value, InterpreterError> {
        let arg = args.first().cloned().ok_or_else(|| InterpreterError::ArityError {
            expected: 1,
            got: args.len(),
            callee: "eval".into(),
            span: Some(span),
        })?;
        let src = match arg {
            Value::Str(s) => (*s).clone(),
            other => {
                return Err(InterpreterError::TypeError {
                    expected: "Str",
                    got: other.type_name(),
                    op: "eval".into(),
                    span: Some(span),
                })
            }
        };
        let output = crate::parser::parse(&src);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::RuntimeError {
                msg: format!("eval parse error: {}", e.full_message()),
                span: Some(span),
            });
        }
        let chunk = crate::compiler::compile_program_with_types(
            &output.program,
            &mut vm.variant_table,
            &mut vm.struct_table,
        )?;
        // Run the compiled chunk on a child frame, sharing `vm.globals` so
        // definitions made inside `eval` persist (matches the tree-walker,
        // which mutates `self.global`). The chunk's arity is 0, so no args
        // are pushed onto the operand stack.
        let stack_base = self.stack.len();
        let target_depth = self.frames.len();
        let saved_loop_len = self.loop_handlers.len();
        let closure = Rc::new(ClosureVm::new(Rc::new(chunk), Vec::new()));
        self.frames.push(Frame {
            closure,
            ip: 0,
            stack_base,
            actor_env: None,
        });
        let result: Result<Value, InterpreterError> = loop {
            if self.frames.len() <= target_depth {
                break Ok(self.stack.pop().unwrap_or(Value::Nil));
            }
            match self.step(vm) {
                Ok(()) => {}
                Err(e) => match self.handle_signal(e, target_depth)? {
                    SignalOutcome::Continue => {}
                    SignalOutcome::Done(v) => break Ok(v),
                },
            }
        };
        self.loop_handlers.truncate(saved_loop_len);
        while self.frames.len() > target_depth {
            let f = self.frames.pop().unwrap();
            self.close_upvalues_above(f.stack_base);
        }
        self.stack.truncate(stack_base);
        result
    }

    // -----------------------------------------------------------------------
    // Higher-order builtins
    // -----------------------------------------------------------------------

    fn ho_map(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        let (coll, f) = two_args_native(args, "map", span)?;
        let f = expect_closure(f, "map", span)?;
        let items = iter_items(&coll, "map", span)?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let r = self.call_closure_sync(vm, f.clone(), vec![item])?;
            out.push(r);
        }
        self.push(Value::Vec(out.into_iter().collect()));
        Ok(())
    }

    fn ho_filter(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        let (coll, f) = two_args_native(args, "filter", span)?;
        let f = expect_closure(f, "filter", span)?;
        let items = iter_items(&coll, "filter", span)?;
        let mut out = Vec::new();
        for item in items {
            let keep = self.call_closure_sync(vm, f.clone(), vec![item.clone()])?;
            if keep.is_truthy() {
                out.push(item);
            }
        }
        match coll {
            Value::Vec(_) => self.push(Value::Vec(out.into_iter().collect())),
            Value::Set(_) => self.push(Value::Set(out.into_iter().collect())),
            other => {
                return Err(InterpreterError::TypeError {
                    expected: "Vec or Set",
                    got: other.type_name(),
                    op: "filter".into(),
                    span: Some(span),
                })
            }
        }
        Ok(())
    }

    fn ho_fold(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        if args.len() != 3 {
            return Err(InterpreterError::ArityError {
                expected: 3,
                got: args.len(),
                callee: "fold".into(),
                span: Some(span),
            });
        }
        // fold(coll, init, fn) — matches tree-walker argument order.
        let coll = args[0].clone();
        let mut acc = args[1].clone();
        let f = expect_closure(args[2].clone(), "fold", span)?;
        let items = iter_items(&coll, "fold", span)?;
        for item in items {
            acc = self.call_closure_sync(vm, f.clone(), vec![acc, item])?;
        }
        self.push(acc);
        Ok(())
    }

    fn ho_reduce(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        if args.len() != 2 {
            return Err(InterpreterError::ArityError {
                expected: 2,
                got: args.len(),
                callee: "reduce".into(),
                span: Some(span),
            });
        }
        // reduce(coll, fn) — matches tree-walker argument order.
        let coll = args[0].clone();
        let f = expect_closure(args[1].clone(), "reduce", span)?;
        let items = iter_items(&coll, "reduce", span)?;
        let mut items = items.into_iter();
        let mut acc = match items.next() {
            Some(v) => v,
            None => {
                return Err(InterpreterError::RuntimeError {
                    msg: "reduce of empty collection".into(),
                    span: Some(span),
                })
            }
        };
        for item in items {
            acc = self.call_closure_sync(vm, f.clone(), vec![acc, item])?;
        }
        self.push(acc);
        Ok(())
    }

    fn ho_find(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        let (coll, f) = two_args_native(args, "find", span)?;
        let f = expect_closure(f, "find", span)?;
        let items = iter_items(&coll, "find", span)?;
        for item in items {
            let found = self.call_closure_sync(vm, f.clone(), vec![item.clone()])?;
            if found.is_truthy() {
                self.push(item);
                return Ok(());
            }
        }
        self.push(Value::Nil);
        Ok(())
    }

    fn ho_each(
        &mut self,
        vm: &mut Vm,
        args: &[Value],
        span: crate::ast::Span,
    ) -> Result<(), InterpreterError> {
        let (coll, f) = two_args_native(args, "each", span)?;
        let f = expect_closure(f, "each", span)?;
        let items = iter_items(&coll, "each", span)?;
        for item in items {
            self.call_closure_sync(vm, f.clone(), vec![item])?;
        }
        self.push(Value::Nil);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Pattern matching
    // -----------------------------------------------------------------------

    fn vm_match(
        &mut self,
        pattern: &Pattern,
        value: &Value,
    ) -> Result<MatchOutcome, InterpreterError> {
        let mut bindings: Vec<Value> = Vec::new();
        if self.match_rec(pattern, value, &mut bindings)? {
            Ok(MatchOutcome::Matched(bindings))
        } else {
            Ok(MatchOutcome::Failed)
        }
    }

    fn match_rec(
        &mut self,
        pattern: &Pattern,
        value: &Value,
        bindings: &mut Vec<Value>,
    ) -> Result<bool, InterpreterError> {
        match pattern {
            Pattern::Wildcard(_) => Ok(true),
            Pattern::Bind(_, _) => {
                bindings.push(value.clone());
                Ok(true)
            }
            Pattern::Lit(lit, _) => {
                let lit_val = match lit {
                    crate::ast::LitPattern::Int(n) => Value::Int(n.clone()),
                    crate::ast::LitPattern::Decimal(d) => Value::Decimal(d.clone()),
                    crate::ast::LitPattern::Bool(b) => Value::Bool(*b),
                    crate::ast::LitPattern::Nil => Value::Nil,
                    crate::ast::LitPattern::Str(parts) => {
                        // Only literal (non-interpolated) string patterns are
                        // supported in the VM; interpolated ones are rare and
                        // would need bytecode evaluation of the parts.
                        let mut s = String::new();
                        for part in parts {
                            match part {
                                crate::ast::StrPart::Literal(t) => s.push_str(t),
                                crate::ast::StrPart::Expr(_) => {
                                    return Err(InterpreterError::RuntimeError {
                                        msg: "interpolated string pattern not supported in VM".into(),
                                        span: None,
                                    })
                                }
                            }
                        }
                        Value::str(s)
                    }
                };
                Ok(&lit_val == value)
            }
            Pattern::Variant { name, args, .. } => match value {
                Value::Variant { name: vn, args: va } => {
                    if vn.as_str() != name.as_str() || args.len() != va.len() {
                        return Ok(false);
                    }
                    for (pat, val) in args.iter().zip(va.iter()) {
                        if !self.match_rec(pat, val, bindings)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
            Pattern::Struct { fields, .. } => match value {
                Value::Struct { fields: vf, .. } => {
                    for (fname, fpat) in fields {
                        match vf.get(fname) {
                            Some(fval) => {
                                if !self.match_rec(fpat, fval, bindings)? {
                                    return Ok(false);
                                }
                            }
                            None => return Ok(false),
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
            Pattern::Vec { pats, rest, .. } => match value {
                Value::Vec(v) => {
                    if *rest {
                        if v.len() < pats.len() {
                            return Ok(false);
                        }
                    } else if v.len() != pats.len() {
                        return Ok(false);
                    }
                    for (i, pat) in pats.iter().enumerate() {
                        if !self.match_rec(pat, &v[i], bindings)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
            Pattern::Or(pats, _) => {
                // Each alternative is tried with a fresh binding buffer; on
                // success the buffer is committed.
                for pat in pats {
                    let mut sub: Vec<Value> = Vec::new();
                    if self.match_rec(pat, value, &mut sub)? {
                        bindings.extend(sub);
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Pattern::Guard(pat, guard, _) => {
                // Nested guards need bytecode evaluation of `guard` after
                // bindings are established. Phase A does not support nested
                // guards inside patterns; use the arm-level `guard` instead.
                // We evaluate the guard by compiling it on the fly is not
                // possible here, so we reject it.
                let _ = (pat, guard);
                Err(InterpreterError::RuntimeError {
                    msg: "nested `pattern if guard` not supported in VM; use arm-level guard".into(),
                    span: None,
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Exceptions / unwinding
    // -----------------------------------------------------------------------

    fn raise_value(&self, v: Value) -> InterpreterError {
        InterpreterError::UserException {
            value: v,
            span: Some(self.current_span()),
        }
    }

    // -----------------------------------------------------------------------
    // Upvalue management
    // -----------------------------------------------------------------------

    fn make_closure(&mut self, chunk_idx: usize) -> Rc<ClosureVm> {
        let f = self.current_closure();
        let sub_chunk = f.chunk.sub_chunks[chunk_idx].clone();
        let upvalue_count = sub_chunk.upvalue_count;
        let mut upvalues = Vec::with_capacity(upvalue_count);
        // Read upvalue descriptors following the Closure operand.
        // Each descriptor: 1 byte is_local, 1 byte index.
        for _ in 0..upvalue_count {
            let is_local = self.read_u8();
            let index = self.read_u8() as usize;
            if is_local == 1 {
                // Capture a local of the *current* frame.
                let base = self.frames[self.frames.len() - 1].stack_base;
                let stack_index = base + index;
                let uv = Rc::new(Upvalue::open(stack_index));
                // Insert into open_upvalues keeping descending order.
                self.insert_open_upvalue(uv.clone());
                upvalues.push(uv);
            } else {
                // Capture an upvalue of the current closure.
                let uv = f.upvalues[index].clone();
                upvalues.push(uv);
            }
        }
        // Inherit the current frame's actor_env so that closures defined
        // inside an actor (handlers, actor-local fns, lambdas) carry the
        // actor's state namespace with them.
        let actor_env = self.frames[self.frames.len() - 1].actor_env.clone();
        let mut closure = ClosureVm::new(sub_chunk, upvalues);
        closure.actor_env = actor_env;
        Rc::new(closure)
    }

    fn insert_open_upvalue(&mut self, uv: UpvalueRef) {
        // Insert sorted by stack_index descending (topmost-first). This lets
        // close_upvalues_above scan linearly from the front.
        let uv_idx = uv.stack_index().unwrap_or(0);
        let pos = self
            .open_upvalues
            .iter()
            .position(|existing| {
                // Descending: insert before the first existing with smaller index.
                match existing.stack_index() {
                    Some(idx) => idx < uv_idx,
                    None => false,
                }
            })
            .unwrap_or(self.open_upvalues.len());
        self.open_upvalues.insert(pos, uv);
    }

    fn close_upvalues_above(&mut self, floor: usize) {
        // open_upvalues is sorted descending; pop from the front while the
        // upvalue points at an index >= floor.
        let mut i = 0;
        while i < self.open_upvalues.len() {
            if self.open_upvalues[i].close_if_at_or_above(&self.stack, floor) {
                self.open_upvalues.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Recursively walk `v` and close all OPEN upvalues of any VM closures
    /// found, using this ctx's stack. Called before a value escapes to another
    /// coroutine (e.g. actor message send `!`), because OPEN upvalues point
    /// into *this* ctx's stack and would read garbage in the receiver's ctx
    /// (which has its own, separate stack).
    ///
    /// Tree-walker `Func` closures capture a heap-allocated `EnvRef` scope
    /// chain (not stack slots), so they need no closing — only `Value::Closure`
    /// (VM closures with Lua-style open/closed upvalues) is affected.
    fn close_escaping_upvalues(&self, v: &Value) {
        match v {
            Value::Closure(c) => {
                for uv in &c.upvalues {
                    if let Some(idx) = uv.stack_index() {
                        // Bounds-check: the upvalue should point into this
                        // ctx's stack. If it points elsewhere (e.g. a closure
                        // already received from another actor whose upvalues
                        // were not closed — a pre-existing bug), skip it
                        // rather than panicking.
                        if idx < self.stack.len() {
                            uv.close_if_at(&self.stack, idx);
                        }
                    }
                }
            }
            Value::Variant { args, .. } => {
                for a in args.iter() {
                    self.close_escaping_upvalues(a);
                }
            }
            Value::Struct { fields, .. } => {
                for (_, a) in fields.iter() {
                    self.close_escaping_upvalues(a);
                }
            }
            Value::Vec(v) => {
                for a in v.iter() {
                    self.close_escaping_upvalues(a);
                }
            }
            Value::Map(m) => {
                for (_, a) in m.iter() {
                    self.close_escaping_upvalues(a);
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Stack / frame access helpers
    // -----------------------------------------------------------------------

    #[inline]
    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    #[inline]
    fn pop(&mut self) -> Value {
        self.stack.pop().expect("stack underflow")
    }

    #[inline]
    fn peek(&self, down: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - down]
    }

    #[inline]
    fn fetch_op(&mut self) -> Result<OpCode, InterpreterError> {
        let idx = self.frames.len() - 1;
        let f = &mut self.frames[idx];
        let b = f.closure.chunk.code[f.ip];
        f.ip += 1;
        opcode_from_u8(b).ok_or_else(|| InterpreterError::RuntimeError {
            msg: format!("invalid opcode {}", b),
            span: Some(self.current_span()),
        })
    }

    #[inline]
    fn read_u8(&mut self) -> u8 {
        let idx = self.frames.len() - 1;
        let f = &mut self.frames[idx];
        read_u8(&f.closure.chunk.code, &mut f.ip)
    }

    #[inline]
    fn read_u16(&mut self) -> u16 {
        let idx = self.frames.len() - 1;
        let f = &mut self.frames[idx];
        read_u16(&f.closure.chunk.code, &mut f.ip)
    }

    #[inline]
    fn set_ip(&mut self, addr: usize) {
        let len = self.frames.len();
        self.frames[len - 1].ip = addr;
    }

    #[inline]
    fn current_closure(&self) -> Rc<ClosureVm> {
        self.frames[self.frames.len() - 1].closure.clone()
    }

    #[inline]
    fn current_span(&self) -> crate::ast::Span {
        let f = &self.frames[self.frames.len() - 1];
        if f.ip == 0 {
            return crate::ast::Span::dummy();
        }
        f.closure.chunk.spans.get(f.ip - 1).cloned().unwrap_or(crate::ast::Span::dummy())
    }

    #[inline]
    fn const_at(&self, idx: usize) -> Value {
        let f = &self.frames[self.frames.len() - 1];
        f.closure.chunk.consts[idx].clone()
    }

    #[inline]
    fn const_name(&self, idx: usize) -> String {
        match self.const_at(idx) {
            Value::Str(s) => (*s).clone(),
            other => panic!("expected string const, got {:?}", other.type_name()),
        }
    }

    #[inline]
    fn pattern_at(&self, idx: usize) -> Rc<Pattern> {
        let f = &self.frames[self.frames.len() - 1];
        f.closure.chunk.patterns[idx].clone()
    }

    #[inline]
    fn local(&self, slot: usize) -> Value {
        let f = &self.frames[self.frames.len() - 1];
        self.stack[f.stack_base + slot].clone()
    }

    #[inline]
    fn set_local(&mut self, slot: usize, v: Value) {
        let base = self.frames[self.frames.len() - 1].stack_base;
        let idx = base + slot;
        if idx < self.stack.len() {
            self.stack[idx] = v;
        } else {
            // Extend (shouldn't normally happen; slots are pre-allocated).
            while self.stack.len() < idx {
                self.stack.push(Value::Nil);
            }
            self.stack.push(v);
        }
    }

    #[inline]
    fn assign_local(&mut self, slot: usize, v: Value) {
        let base = self.frames[self.frames.len() - 1].stack_base;
        let idx = base + slot;
        // Write-through SharedRef, matching the tree-walker.
        if let Value::Shared(sref) = &self.stack[idx] {
            let sref = sref.clone();
            self.write_shared(sref, v);
        } else {
            self.stack[idx] = v;
        }
    }

    fn write_shared(&mut self, sref: SharedRef, v: Value) {
        if let Some(ctx) = self.txn_stack.last_mut() {
            // Inside a transaction: buffer the write.
            let id = TransactionContext::ref_id(&sref);
            ctx.writes.insert(id, (sref, v));
        } else {
            // Outside: direct write + version bump.
            let mut cell = sref.borrow_mut();
            cell.value = v;
            cell.version += 1;
        }
    }

    /// Read a value, dereferencing SharedRef. Inside a transaction, check
    /// the write-set first (snapshot isolation), then read the committed
    /// value and record its version. Outside a transaction, just return
    /// the current value.
    fn read_shared(&mut self, v: Value) -> Value {
        match v {
            Value::Shared(sref) => {
                if self.txn_stack.is_empty() {
                    sref.borrow().value.clone()
                } else {
                    let id = TransactionContext::ref_id(&sref);
                    // Check write-sets from innermost to outermost.
                    for ctx in self.txn_stack.iter().rev() {
                        if let Some((_, v)) = ctx.writes.get(&id) {
                            return v.clone();
                        }
                    }
                    // Not in any write-set — read committed value.
                    let (value, version) = {
                        let cell = sref.borrow();
                        (cell.value.clone(), cell.version)
                    };
                    // Record read version in the innermost context (keep
                    // the earliest version).
                    if let Some(ctx) = self.txn_stack.last_mut() {
                        ctx.reads.entry(id).or_insert((sref, version));
                    }
                    value
                }
            }
            other => other,
        }
    }

    /// Read a SharedRef's committed value without transaction bookkeeping.
    /// Used by `commit_txn` validation (which compares recorded read versions
    /// against current committed versions).
    fn shared_committed_version(sref: &SharedRef) -> u64 {
        sref.borrow().version
    }

    /// Pop any transact handlers (and their contexts) whose `stack_depth` is
    /// strictly greater than `floor`. Called during exception/retry unwinding
    /// to roll back transactions that are being exited.
    fn cleanup_transact_above(&mut self, floor: usize) {
        while let Some(h) = self.transact_handlers.last() {
            if h.stack_depth > floor {
                self.transact_handlers.pop();
                self.txn_stack.pop();
            } else {
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Binary op helpers
    // -----------------------------------------------------------------------

    fn binop<F>(&mut self, f: F) -> Result<(), InterpreterError>
    where
        F: FnOnce(&Value, &Value) -> Result<Value, InterpreterError>,
    {
        let b = self.pop();
        let a = self.pop();
        self.push(f(&a, &b)?);
        Ok(())
    }

    fn cmp_op<F>(&mut self, f: F) -> Result<(), InterpreterError>
    where
        F: FnOnce(&Value, &Value) -> Result<bool, InterpreterError>,
    {
        let b = self.pop();
        let a = self.pop();
        let r = f(&a, &b)?;
        self.push(Value::Bool(r));
        Ok(())
    }

    fn do_index(&self, target: &Value, idx: &Value) -> Result<Value, InterpreterError> {
        match target {
            Value::Vec(v) => {
                if let Value::Int(n) = idx {
                    let i = n.to_isize().ok_or_else(|| InterpreterError::IndexError {
                        msg: format!("non-integer index: {}", idx),
                        span: None,
                    })?;
                    if i < 0 || i as usize >= v.len() {
                        return Err(InterpreterError::IndexError {
                            msg: format!("index {} out of bounds (len {})", i, v.len()),
                            span: None,
                        });
                    }
                    Ok(v[i as usize].clone())
                } else {
                    Err(InterpreterError::IndexError {
                        msg: format!("vec index must be Int, got {}", idx.type_name()),
                        span: None,
                    })
                }
            }
            Value::Map(m) => Ok(m.get(idx).cloned().unwrap_or(Value::Nil)),
            _ => Err(InterpreterError::TypeError {
                expected: "Vec or Map",
                got: target.type_name(),
                op: "index".into(),
                span: None,
            }),
        }
    }

    fn do_field(&self, target: &Value, name: &str) -> Result<Value, InterpreterError> {
        // Auto-deref SharedRef so `shared_map.field` reads the inner
        // value's field (reference semantics for shared cells).
        let target = match target {
            Value::Shared(sref) => &sref.borrow().value,
            other => other,
        };
        match target {
            // Struct field access is strict (missing field raises IndexError),
            // mirroring the tree-walker. Map field access is lenient (missing
            // key returns Nil).
            Value::Struct { fields, .. } => fields.get(name).cloned().ok_or_else(|| {
                InterpreterError::IndexError {
                    msg: format!("no field `{}`", name),
                    span: Some(self.current_span()),
                }
            }),
            Value::Map(m) => {
                let key = Value::str(name);
                Ok(m.get(&key).cloned().unwrap_or(Value::Nil))
            }
            Value::Module(module) => module
                .exports
                .get(name)
                .cloned()
                .ok_or_else(|| InterpreterError::ImportError {
                    path: module.name.clone(),
                    msg: format!("no export `{}`", name),
                    span: Some(self.current_span()),
                }),
            _ => Err(InterpreterError::TypeError {
                expected: "Struct, Map, or Module",
                got: target.type_name(),
                op: format!(".{}", name),
                span: None,
            }),
        }
    }
}

impl Default for VmCtx {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

pub fn opcode_from_u8(b: u8) -> Option<OpCode> {
    // OpCode is `#[repr(u8)]` with explicit discriminants starting at 0 and
    // ending at `_End`. We bounds-check then transmute — this avoids the
    // manual match table drifting out of sync with the enum definition
    // (which previously caused a miscompile where `DefineActor` (67) was
    // being decoded as `PushLoop`).
    let end = OpCode::_End as u8;
    if b >= end {
        return None;
    }
    // SAFETY: `OpCode` is `#[repr(u8)]`, so every `u8` in `0.._End` is a
    // valid discriminant.
    Some(unsafe { std::mem::transmute(b) })
}

fn two_args_native(
    args: &[Value],
    name: &str,
    span: crate::ast::Span,
) -> Result<(Value, Value), InterpreterError> {
    if args.len() != 2 {
        return Err(InterpreterError::ArityError {
            expected: 2,
            got: args.len(),
            callee: name.into(),
            span: Some(span),
        });
    }
    Ok((args[0].clone(), args[1].clone()))
}

fn expect_closure(
    v: Value,
    name: &str,
    span: crate::ast::Span,
) -> Result<Rc<ClosureVm>, InterpreterError> {
    match v {
        Value::Closure(c) => Ok(c),
        Value::Func(_) => Err(InterpreterError::TypeError {
            expected: "VM closure",
            got: "tree-walker closure",
            op: name.into(),
            span: Some(span),
        }),
        other => Err(InterpreterError::TypeError {
            expected: "function",
            got: other.type_name(),
            op: name.into(),
            span: Some(span),
        }),
    }
}

fn iter_items(
    coll: &Value,
    name: &str,
    span: crate::ast::Span,
) -> Result<Vec<Value>, InterpreterError> {
    match coll {
        Value::Vec(v) => Ok(v.iter().cloned().collect()),
        Value::Set(s) => Ok(s.iter().cloned().collect()),
        Value::Map(m) => Ok(m
            .iter()
            .map(|(k, v)| {
                let mut entry = im::HashMap::new();
                entry.insert(Value::str("key"), k.clone());
                entry.insert(Value::str("value"), v.clone());
                Value::Map(entry)
            })
            .collect()),
        _ => Err(InterpreterError::TypeError {
            expected: "Vec, Set, or Map",
            got: coll.type_name(),
            op: name.into(),
            span: Some(span),
        }),
    }
}

/// Negate a numeric value (Int or Decimal).
fn vm_neg(v: &Value) -> Result<Value, InterpreterError> {
    match v {
        Value::Int(n) => Ok(Value::Int(-n.clone())),
        Value::Decimal(d) => Ok(Value::Decimal(-d.clone())),
        _ => Err(InterpreterError::TypeError {
            expected: "Int or Decimal",
            got: v.type_name(),
            op: "neg".into(),
            span: None,
        }),
    }
}

/// Build a namespace Module tree from sub-segments wrapping `leaf`.
/// `sub_segments = ["b", "c"]`, `leaf = c_module`, `root_name = "a"`
/// → Module { name: "a", exports: { "b": Module { "c": c_module } } }.
fn build_namespace_module(
    sub_segments: &[&str],
    leaf: crate::value::ModuleRef,
    root_name: &str,
) -> crate::value::ModuleRef {
    let mut current = leaf;
    for (i, &key) in sub_segments.iter().enumerate().rev() {
        let name = if i == 0 {
            root_name.to_string()
        } else {
            sub_segments[i - 1].to_string()
        };
        let mut exports = std::collections::HashMap::new();
        exports.insert(key.to_string(), Value::Module(current));
        current = Rc::new(ModuleData {
            name,
            source_path: None,
            exports,
        });
    }
    current
}

/// Merge a new sub-path into an existing namespace Module. Returns a new
/// ModuleRef (ModuleData is immutable behind Rc, so we clone exports).
fn merge_namespace(
    existing: &crate::value::ModuleRef,
    sub_segments: &[&str],
    leaf: crate::value::ModuleRef,
) -> crate::value::ModuleRef {
    let mut exports = existing.exports.clone();
    let key = sub_segments[0].to_string();

    if sub_segments.len() == 1 {
        exports.insert(key, Value::Module(leaf));
    } else {
        let sub = match exports.get(&key) {
            Some(Value::Module(m)) => merge_namespace(m, &sub_segments[1..], leaf),
            _ => build_namespace_module(&sub_segments[1..], leaf, &key),
        };
        exports.insert(key, Value::Module(sub));
    }

    Rc::new(ModuleData {
        name: existing.name.clone(),
        source_path: existing.source_path.clone(),
        exports,
    })
}

// Re-export the chunk writers for the compiler module.
pub use crate::vm::chunk::{write_u8 as emit_u8, write_u16 as emit_u16};
