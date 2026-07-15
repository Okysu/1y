//! Tree-walking interpreter for the `1y` language (Phase 3).
//!
//! Evaluates a parsed [`Program`] by walking the AST. Each expression
//! evaluates to a [`Value`]; each statement mutates the environment.
//!
//! # Supported features
//!
//! - Arbitrary-precision arithmetic with auto-promotion (`Int` → `Decimal`)
//! - Persistent collections (`Vec`/`Map`/`Set`/`List`) via `im`
//! - Lexical closures, lambda, named functions
//! - Pattern matching (wildcard, literal, bind, variant, struct, vec, or, guard)
//! - `if`/`match`/`try`/`raise`/`return`
//! - Pipe operator `|>`
//! - Struct/enum construction and destructuring
//! - Actors (`spawn`/`!`/`?`/`on`/`state`) with single-threaded event loop
//! - Transactional memory (`shared`/`transact`/`retry`) with COW + versioning
//!
//! # Not yet supported (later phases)
//!
//! - Module system (`import`)
//! - Methods (dispatched by receiver type)

pub mod builtins;
pub mod env;
pub mod error;
pub mod ops;
pub mod stdlib;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::ast::*;
use crate::interpreter::env::{Environment, EnvRef};
use crate::interpreter::error::InterpreterError;
use crate::value::{ActorInstance, ActorRef, Closure, Envelope, ModuleRef, SharedRef, Value};
use num_traits::ToPrimitive;

// ---------------------------------------------------------------------------
// Transaction context (Phase 3)
// ---------------------------------------------------------------------------

/// One transaction's local state: buffered writes + recorded read versions.
///
/// Writes are keyed by the `SharedRef`'s raw pointer so that the same `shared`
/// binding always maps to the same entry. Reads record the version observed
/// at first read; on commit these are validated against the current versions
/// to detect conflicts.
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

/// Outcome of committing a transaction context.
enum CommitOutcome {
    /// Committed (outermost) or merged (nested); carries the result value.
    Done(Value),
    /// A read-conflict was detected at the outermost level — retry.
    Conflict,
}

/// Extract exactly two arguments from a builtin call, or return an arity error.
fn two_args(args: &[Value], name: &str, span: Span) -> Result<(Value, Value), InterpreterError> {
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

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

/// The tree-walking evaluator. Holds the global environment and registries
/// for user-defined types (enums + structs) so that constructor calls like
/// `Some(42)` or `Point({ x: 1, y: 2 })` can be resolved during `Call`.
pub struct Interpreter {
    global: EnvRef,
    /// Enum variant name → (enum name, field count).
    /// Used to construct `Value::Variant` when a variant name is called.
    variants: HashMap<String, (String, usize)>,
    /// Struct type name → field names (for validation / future use).
    structs: HashMap<String, Vec<String>>,
    /// Actor definitions: `actor Name { state ...; on ...; }`.
    actors: HashMap<String, ActorDef>,
    /// All live actor instances, used by the event loop to drain `!` messages.
    live_actors: Vec<ActorRef>,
    /// Stack of active transaction contexts (innermost = last).
    /// Non-empty only during evaluation of a `transact { ... }` body.
    txn_stack: Vec<TransactionContext>,
    // --- Phase 4: module system ---
    /// Pre-built standard library modules, keyed by name ("io", "env", ...).
    std_modules: HashMap<String, ModuleRef>,
    /// Cache of loaded file modules, keyed by canonical file path.
    module_cache: HashMap<PathBuf, ModuleRef>,
    /// Stack of modules currently being loaded (for circular-import detection).
    module_load_stack: Vec<PathBuf>,
    /// Directory of the entry file, used for relative `import` resolution.
    entry_dir: Option<PathBuf>,
    /// Async scheduler (Phase 4.7: Zig-style colorless async).
    /// Holds coroutines for actor handlers that may `await`.
    scheduler: crate::runtime::scheduler::Scheduler,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    /// Create a fresh interpreter with builtins registered.
    pub fn new() -> Self {
        let global = Environment::global();
        builtins::register(&global);
        Interpreter {
            global,
            variants: HashMap::new(),
            structs: HashMap::new(),
            actors: HashMap::new(),
            live_actors: Vec::new(),
            txn_stack: Vec::new(),
            std_modules: stdlib::build_std_modules(),
            module_cache: HashMap::new(),
            module_load_stack: Vec::new(),
            entry_dir: None,
            scheduler: crate::runtime::scheduler::Scheduler::new(),
        }
    }

    /// Set the entry-file directory, used for relative `import` resolution.
    pub fn set_entry_dir(&mut self, dir: PathBuf) {
        self.entry_dir = Some(dir);
    }

    /// Parse and evaluate a source string.
    pub fn run(&mut self, source: &str) -> Result<(), InterpreterError> {
        let output = crate::parser::parse(source);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::RuntimeError {
                msg: format!("parse error: {}", e.message),
                span: Some(e.span),
            });
        }
        let result = self.eval_program(&output.program);
        // Drain pending `!` messages regardless of whether the main program
        // succeeded — actors should still process fire-and-forget messages
        // that were queued before the error/exit.
        let _ = self.drain_mailboxes();
        result
    }

    /// Evaluate a whole program (sequence of top-level statements).
    pub fn eval_program(&mut self, program: &Program) -> Result<(), InterpreterError> {
        for stmt in &program.stmts {
            self.eval_stmt(&self.global.clone(), stmt)?;
        }
        Ok(())
    }

    /// Parse and evaluate `source`, returning the value of the last top-level
    /// expression (or `Nil` if the last statement is not an expression).
    /// Useful for testing and REPL-style usage.
    pub fn eval_source(&mut self, source: &str) -> Result<Value, InterpreterError> {
        let output = crate::parser::parse(source);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::RuntimeError {
                msg: format!("parse error: {}", e.message),
                span: Some(e.span),
            });
        }
        let mut last = Value::Nil;
        for stmt in &output.program.stmts {
            match stmt {
                Stmt::Expr(e) => {
                    last = self.eval_expr(&self.global.clone(), e)?;
                }
                _ => {
                    self.eval_stmt(&self.global.clone(), stmt)?;
                }
            }
        }
        Ok(last)
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    pub fn eval_stmt(&mut self, env: &EnvRef, stmt: &Stmt) -> Result<(), InterpreterError> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let v = self.eval_expr(env, value)?;
                env.borrow_mut().define(name.clone(), v);
                Ok(())
            }
            Stmt::SharedDecl { name, value, .. } => {
                // Phase 3: create a versioned SharedRef and bind it.
                let v = self.eval_expr(env, value)?;
                let cell = crate::value::SharedCell { value: v, version: 0 };
                let sref: SharedRef = Rc::new(RefCell::new(cell));
                env.borrow_mut().define(name.clone(), Value::Shared(sref));
                Ok(())
            }
            Stmt::FuncDef(fd) => {
                let closure = Closure {
                    params: fd.params.clone(),
                    body: (*fd.body).clone(),
                    env: env.clone(),
                    name: Some(fd.name.clone()),
                };
                env.borrow_mut().define(fd.name.clone(), Value::Func(Rc::new(closure)));
                Ok(())
            }
            Stmt::TypeDef(td) => {
                let field_names: Vec<String> =
                    td.fields.iter().map(|(n, _)| n.clone()).collect();
                self.structs.insert(td.name.clone(), field_names);
                Ok(())
            }
            Stmt::EnumDef(ed) => {
                for variant in &ed.variants {
                    let arity = variant.fields.len();
                    self.variants
                        .insert(variant.name.clone(), (ed.name.clone(), arity));
                    // Zero-arity variants are defined as values so they can be
                    // referenced directly (e.g. `None`).
                    if arity == 0 {
                        env.borrow_mut().define(
                            variant.name.clone(),
                            Value::Variant {
                                name: Rc::new(variant.name.clone()),
                                args: Rc::new(vec![]),
                            },
                        );
                    }
                }
                Ok(())
            }
            Stmt::Expr(expr) => {
                self.eval_expr(env, expr)?;
                Ok(())
            }
            Stmt::Semi(expr) => {
                self.eval_expr(env, expr)?;
                Ok(())
            }
            Stmt::ActorDef(ad) => {
                // Register the actor definition for later `spawn`.
                self.actors.insert(ad.name.clone(), ad.clone());
                Ok(())
            }
            Stmt::StateDecl { span, .. } | Stmt::OnClause(OnClause { span, .. }) => {
                // `state` and `on` are only meaningful inside an `actor` body,
                // which is consumed by `spawn`. Reaching here means they were
                // written at top level, which is a syntax-level mistake.
                Err(InterpreterError::RuntimeError {
                    msg: "`state`/`on` may only appear inside an `actor` body".into(),
                    span: Some(*span),
                })
            }
            Stmt::Import(Import { path, alias, lazy, span }) => {
                self.eval_import(env, path, alias.as_deref(), *lazy, *span)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Module system (Phase 4)
    // -----------------------------------------------------------------------

    /// Handle `import path` / `import path as alias` / `lazy import path`.
    ///
    /// For eager import: load the module and bind it under the alias (or the
    /// last segment of the dotted path if no alias).
    ///
    /// For lazy import: bind a `LazyImport` placeholder that is resolved on
    /// first access (see `Expr::Ident`).
    fn eval_import(
        &mut self,
        env: &EnvRef,
        path: &str,
        alias: Option<&str>,
        lazy: bool,
        span: Span,
    ) -> Result<(), InterpreterError> {
        // Determine the binding name: explicit alias, or last segment of path.
        let bind_name = match alias {
            Some(a) => a.to_string(),
            None => {
                // `import a.b.c` binds `c`.
                path.rsplit('.').next().unwrap_or(path).to_string()
            }
        };

        if lazy {
            // Defer: bind a LazyImport placeholder.
            env.borrow_mut()
                .define(bind_name, Value::LazyImport { path: Rc::new(path.to_string()) });
            return Ok(());
        }

        // Eager: load immediately.
        let module = self.load_module(path, span)?;
        env.borrow_mut().define(bind_name, Value::Module(module));
        Ok(())
    }

    /// Load a module by dotted path. Checks std modules first, then file paths.
    fn load_module(&mut self, path: &str, span: Span) -> Result<ModuleRef, InterpreterError> {
        // 1. Standard library module?
        if let Some(m) = self.std_modules.get(path) {
            return Ok(m.clone());
        }

        // 2. File module: resolve `path.1y` relative to entry dir.
        let file_path = self.resolve_module_path(path);
        let canonical = match file_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return Err(InterpreterError::ImportError {
                    path: path.to_string(),
                    msg: format!("module file not found: {:?}", file_path),
                    span: Some(span),
                });
            }
        };

        // 3. Check cache.
        if let Some(m) = self.module_cache.get(&canonical) {
            return Ok(m.clone());
        }

        // 4. Circular-import detection.
        if self.module_load_stack.iter().any(|p| p == &canonical) {
            let chain = self
                .module_load_stack
                .iter()
                .map(|p| p.display().to_string())
                .chain(std::iter::once(canonical.display().to_string()))
                .collect::<Vec<_>>()
                .join(" → ");
            return Err(InterpreterError::ImportError {
                path: path.to_string(),
                msg: format!("circular import: {}", chain),
                span: Some(span),
            });
        }

        // 5. Read and parse the file.
        let source = std::fs::read_to_string(&canonical).map_err(|e| InterpreterError::ImportError {
            path: path.to_string(),
            msg: format!("failed to read module: {}", e),
            span: Some(span),
        })?;

        let output = crate::parser::parse(&source);
        if !output.errors.is_empty() {
            let e = &output.errors[0];
            return Err(InterpreterError::ImportError {
                path: path.to_string(),
                msg: format!("parse error: {}", e.message),
                span: Some(span),
            });
        }

        // 6. Evaluate the module in a fresh child environment.
        let module_env = Environment::child(&self.global);
        self.module_load_stack.push(canonical.clone());
        for stmt in &output.program.stmts {
            self.eval_stmt(&module_env, stmt)?;
        }
        self.module_load_stack.pop();

        // 7. Collect top-level bindings as exports.
        let exports = module_env.borrow().bindings_clone();

        let module = Rc::new(crate::value::ModuleData {
            name: path.to_string(),
            source_path: Some(canonical.clone()),
            exports,
        });

        self.module_cache.insert(canonical, module.clone());
        Ok(module)
    }

    /// Resolve a dotted module path to a file path.
    ///
    /// `a.b.c` → `<entry_dir>/a/b/c.1y` (if entry_dir is set)
    ///         → `a/b/c.1y` (relative to CWD otherwise)
    fn resolve_module_path(&self, path: &str) -> PathBuf {
        let relative: PathBuf = path.split('.').collect();
        let with_ext = relative.with_extension("1y");
        match &self.entry_dir {
            Some(dir) => dir.join(&with_ext),
            None => with_ext,
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    pub fn eval_expr(&mut self, env: &EnvRef, expr: &Expr) -> Result<Value, InterpreterError> {
        match expr {
            // --- literals ---
            Expr::Int(n, _) => Ok(Value::Int(n.clone())),
            Expr::Decimal(d, _) => Ok(Value::Decimal(d.clone())),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Nil(_) => Ok(Value::Nil),
            Expr::Str(str_lit, _) => {
                let mut result = String::new();
                for part in &str_lit.parts {
                    match part {
                        StrPart::Literal(s) => result.push_str(s),
                        StrPart::Expr(e) => {
                            let v = self.eval_expr(env, e)?;
                            // Interpolated strings insert the raw content of a
                            // Str value (no surrounding quotes), matching the
                            // convention in JS template literals / Python f-strings.
                            match &v {
                                Value::Str(s) => result.push_str(s),
                                _ => result.push_str(&format!("{}", v)),
                            }
                        }
                    }
                }
                Ok(Value::str(result))
            }
            Expr::Ident(name, span) => {
                let val = env.borrow().get(name).ok_or_else(|| {
                    InterpreterError::NameError {
                        name: name.clone(),
                        span: Some(*span),
                    }
                })?;
                // Lazy import: load on first access and rebind.
                if let Value::LazyImport { path } = &val {
                    let p = (**path).clone();
                    let module = self.load_module(&p, *span)?;
                    env.borrow_mut().define(name.clone(), Value::Module(module.clone()));
                    return Ok(Value::Module(module));
                }
                // Auto-deref SharedRef: inside a transaction, check the
                // write-set first (snapshot isolation), then read the
                // committed value and record its version. Outside a
                // transaction, just return the current value.
                if let Value::Shared(sref) = &val {
                    let id = TransactionContext::ref_id(sref);
                    // Check write-sets from innermost to outermost.
                    for ctx in self.txn_stack.iter().rev() {
                        if let Some((_, v)) = ctx.writes.get(&id) {
                            return Ok(v.clone());
                        }
                    }
                    // Not in any write-set — read committed value.
                    let cell = sref.borrow();
                    let version = cell.version;
                    let value = cell.value.clone();
                    // Record read version in the innermost context (if not
                    // already recorded — keep the earliest version).
                    if let Some(ctx) = self.txn_stack.last_mut() {
                        ctx.reads.entry(id).or_insert((sref.clone(), version));
                    }
                    Ok(value)
                } else {
                    Ok(val)
                }
            }

            // --- collections ---
            Expr::VecLit { items, .. } => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.eval_expr(env, item)?);
                }
                Ok(Value::vec(vals))
            }
            Expr::MapLit { entries, .. } => {
                let mut pairs = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    // Shorthand: a bare identifier key `{ x: 1 }` is treated as
                    // the string `"x"` (like JS/Rust struct-literal shorthand),
                    // not as a variable lookup.
                    let kv = match k {
                        Expr::Ident(name, _) => Value::str(name),
                        _ => self.eval_expr(env, k)?,
                    };
                    let vv = self.eval_expr(env, v)?;
                    pairs.push((kv, vv));
                }
                Ok(Value::map(pairs))
            }
            Expr::SetLit { items, .. } => {
                let mut vals = Vec::with_capacity(items.len());
                for item in items {
                    vals.push(self.eval_expr(env, item)?);
                }
                Ok(Value::set(vals))
            }

            Expr::Paren(inner, _) => self.eval_expr(env, inner),

            // --- arithmetic / comparison / logic ---
            Expr::BinOp { op, lhs, rhs, span } => {
                // Short-circuit evaluation for `and` / `or`.
                match op {
                    BinOp::And => {
                        let l = self.eval_expr(env, lhs)?;
                        if !l.is_truthy() {
                            return Ok(l);
                        }
                        return self.eval_expr(env, rhs);
                    }
                    BinOp::Or => {
                        let l = self.eval_expr(env, lhs)?;
                        if l.is_truthy() {
                            return Ok(l);
                        }
                        return self.eval_expr(env, rhs);
                    }
                    _ => {}
                }
                let l = self.eval_expr(env, lhs)?;
                let r = self.eval_expr(env, rhs)?;
                let result = match op {
                    BinOp::Add => {
                        // String concatenation for Str + Str.
                        if let (Value::Str(a), Value::Str(b)) = (&l, &r) {
                            Ok(Value::str(format!("{}{}", a, b)))
                        } else {
                            ops::add(&l, &r)
                        }
                    }
                    BinOp::Sub => ops::sub(&l, &r),
                    BinOp::Mul => ops::mul(&l, &r),
                    BinOp::Div => ops::div(&l, &r),
                    BinOp::Mod => ops::modulo(&l, &r),
                    BinOp::Eq => Ok(Value::Bool(l == r)),
                    BinOp::Neq => Ok(Value::Bool(l != r)),
                    BinOp::Lt => ops::lt(&l, &r).map(Value::Bool),
                    BinOp::Gt => ops::gt(&l, &r).map(Value::Bool),
                    BinOp::Lte => ops::lte(&l, &r).map(Value::Bool),
                    BinOp::Gte => ops::gte(&l, &r).map(Value::Bool),
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                result.map_err(|e| e.with_span(*span))
            }

            Expr::UnaryOp { op, expr: inner, span } => {
                let v = self.eval_expr(env, inner)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int(n) => Ok(Value::Int(-n)),
                        Value::Decimal(d) => Ok(Value::Decimal(-d)),
                        _ => Err(InterpreterError::TypeError {
                            expected: "number",
                            got: v.type_name(),
                            op: "negate".into(),
                            span: Some(*span),
                        }),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!v.is_truthy())),
                }
            }

            // --- pipe ---
            Expr::Pipe { lhs, rhs, span } => {
                let lhs_val = self.eval_expr(env, lhs)?;
                match rhs.as_ref() {
                    // `a |> f(b)` ≡ `f(a, b)`
                    Expr::Call {
                        callee,
                        args,
                        span: call_span,
                    } => {
                        let func = self.eval_expr(env, callee)?;
                        let mut arg_vals = vec![lhs_val];
                        for a in args {
                            arg_vals.push(self.eval_expr(env, a)?);
                        }
                        self.call_function(&func, arg_vals, *call_span)
                    }
                    // `a |> f` ≡ `f(a)`
                    _ => {
                        let func = self.eval_expr(env, rhs)?;
                        self.call_function(&func, vec![lhs_val], *span)
                    }
                }
            }

            // --- control flow ---
            Expr::If {
                cond, then, else_, ..
            } => {
                let c = self.eval_expr(env, cond)?;
                if c.is_truthy() {
                    self.eval_expr(env, then)
                } else if let Some(e) = else_ {
                    self.eval_expr(env, e)
                } else {
                    Ok(Value::Nil)
                }
            }

            Expr::Match {
                scrutinee, arms, span, ..
            } => {
                let val = self.eval_expr(env, scrutinee)?;
                for arm in arms {
                    let arm_env = Environment::child(env);
                    if self.match_pattern(&arm.pattern, &val, &arm_env)? {
                        // Check arm-level guard (separate from Pattern::Guard).
                        let guard_ok = if let Some(guard) = &arm.guard {
                            self.eval_expr(&arm_env, guard)?.is_truthy()
                        } else {
                            true
                        };
                        if guard_ok {
                            return self.eval_expr(&arm_env, &arm.body);
                        }
                    }
                }
                Err(InterpreterError::PatternMatchFail {
                    value: val,
                    span: Some(*span),
                })
            }

            Expr::Block { stmts, tail, .. } => {
                let block_env = Environment::child(env);
                for stmt in stmts {
                    self.eval_stmt(&block_env, stmt)?;
                }
                match tail {
                    Some(t) => self.eval_expr(&block_env, t),
                    None => Ok(Value::Nil),
                }
            }

            // --- functions ---
            Expr::Lambda {
                params, body, span, ..
            } => {
                let closure = Closure {
                    params: params.clone(),
                    body: (**body).clone(),
                    env: env.clone(),
                    name: None,
                };
                let _ = span; // span not needed for closure construction
                Ok(Value::Func(Rc::new(closure)))
            }

            Expr::Call { callee, args, span } => {
                // Check for enum variant constructor: `Some(42)`.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some((_, arity)) = self.variants.get(name) {
                        if *arity != args.len() {
                            return Err(InterpreterError::ArityError {
                                expected: *arity,
                                got: args.len(),
                                callee: name.clone(),
                                span: Some(*span),
                            });
                        }
                        let mut arg_vals = Vec::with_capacity(args.len());
                        for a in args {
                            arg_vals.push(self.eval_expr(env, a)?);
                        }
                        return Ok(Value::Variant {
                            name: Rc::new(name.clone()),
                            args: Rc::new(arg_vals),
                        });
                    }
                    // Check for struct constructor: `Point({ x: 1, y: 2 })`.
                    if self.structs.contains_key(name) {
                        if args.len() != 1 {
                            return Err(InterpreterError::ArityError {
                                expected: 1,
                                got: args.len(),
                                callee: name.clone(),
                                span: Some(*span),
                            });
                        }
                        let map_val = self.eval_expr(env, &args[0])?;
                        let fields = match &map_val {
                            Value::Map(m) => {
                                let mut fields = HashMap::new();
                                for (k, v) in m.iter() {
                                    if let Value::Str(s) = k {
                                        fields.insert((**s).clone(), v.clone());
                                    }
                                }
                                fields
                            }
                            _ => {
                                return Err(InterpreterError::TypeError {
                                    expected: "Map",
                                    got: map_val.type_name(),
                                    op: format!("constructing {}", name),
                                    span: Some(*span),
                                })
                            }
                        };
                        return Ok(Value::Struct {
                            name: Rc::new(name.clone()),
                            fields: Rc::new(fields),
                        });
                    }
                }
                // Regular function call.
                let func = self.eval_expr(env, callee)?;
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    arg_vals.push(self.eval_expr(env, a)?);
                }
                self.call_function(&func, arg_vals, *span)
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
                span,
            } => {
                let recv_val = self.eval_expr(env, receiver)?;
                // Module method call: `io.read_line(args)` — look up the
                // method in the module's exports, call WITHOUT the receiver.
                if let Value::Module(m) = &recv_val {
                    let func = m.exports.get(method).cloned().ok_or_else(|| {
                        InterpreterError::ImportError {
                            path: m.name.clone(),
                            msg: format!("no export `{}`", method),
                            span: Some(*span),
                        }
                    })?;
                    let mut arg_vals = Vec::with_capacity(args.len());
                    for a in args {
                        arg_vals.push(self.eval_expr(env, a)?);
                    }
                    return self.call_function(&func, arg_vals, *span);
                }
                // Regular method call: desugar `recv.method(args)` →
                // `method(recv, args...)`.
                let func = env.borrow().get(method).ok_or_else(|| {
                    InterpreterError::NameError {
                        name: method.clone(),
                        span: Some(*span),
                    }
                })?;
                let mut arg_vals = vec![recv_val];
                for a in args {
                    arg_vals.push(self.eval_expr(env, a)?);
                }
                self.call_function(&func, arg_vals, *span)
            }

            Expr::Index {
                target, index, span, ..
            } => {
                let coll = self.eval_expr(env, target)?;
                let idx = self.eval_expr(env, index)?;
                ops::get(&coll, &idx).map_err(|e| e.with_span(*span))
            }

            Expr::Field {
                target, name, span, ..
            } => {
                let obj = self.eval_expr(env, target)?;
                match &obj {
                    Value::Struct { fields, .. } => fields.get(name).cloned().ok_or_else(|| {
                        InterpreterError::IndexError {
                            msg: format!("no field `{}`", name),
                            span: Some(*span),
                        }
                    }),
                    Value::Map(m) => {
                        // Field access on maps: `m.field` ≡ `get(m, "field")`.
                        let key = Value::str(name);
                        Ok(m.get(&key).cloned().unwrap_or(Value::Nil))
                    }
                    Value::Module(m) => {
                        // Module field access: `io.read_line` returns the export.
                        m.exports.get(name).cloned().ok_or_else(|| {
                            InterpreterError::ImportError {
                                path: m.name.clone(),
                                msg: format!("no export `{}`", name),
                                span: Some(*span),
                            }
                        })
                    }
                    _ => Err(InterpreterError::TypeError {
                        expected: "Struct, Map, or Module",
                        got: obj.type_name(),
                        op: format!("field access `.{}", name),
                        span: Some(*span),
                    }),
                }
            }

            // --- assignment ---
            Expr::Assign {
                target, value, span, ..
            } => {
                let val = self.eval_expr(env, value)?;
                self.assign_to(env, target, val)
                    .map_err(|e| e.with_span(*span))?;
                Ok(Value::Nil)
            }
            Expr::CompoundAssign {
                op,
                target,
                value,
                span,
                ..
            } => {
                // `target op= value` ≡ `target = target op value`
                let cur = self.eval_expr(env, target)?;
                let rhs = self.eval_expr(env, value)?;
                let new_val = self.apply_binop(*op, &cur, &rhs)
                    .map_err(|e| e.with_span(*span))?;
                self.assign_to(env, target, new_val)
                    .map_err(|e| e.with_span(*span))?;
                Ok(Value::Nil)
            }

            // --- loops ---
            Expr::While { cond, body, .. } => {
                loop {
                    let c = self.eval_expr(env, cond)?;
                    if !c.is_truthy() {
                        break;
                    }
                    match self.eval_expr(env, body) {
                        Ok(_) => {}
                        Err(InterpreterError::Break { value, .. }) => {
                            // break exits the loop; value is the result but
                            // `while` returns nil (consistent with statement semantics).
                            let _ = value;
                            break;
                        }
                        Err(InterpreterError::Continue { .. }) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Nil)
            }
            Expr::For { var, iter, body, span, .. } => {
                let iter_val = self.eval_expr(env, iter)?;
                let items = self.iter_to_vec(&iter_val)
                    .map_err(|e| e.with_span(*span))?;
                for item in items {
                    let loop_env = Environment::child(env);
                    loop_env.borrow_mut().define(var.clone(), item);
                    match self.eval_expr(&loop_env, body) {
                        Ok(_) => {}
                        Err(InterpreterError::Break { value, .. }) => {
                            let _ = value;
                            break;
                        }
                        Err(InterpreterError::Continue { .. }) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Nil)
            }
            Expr::Loop { body, .. } => {
                loop {
                    match self.eval_expr(env, body) {
                        Ok(_) => {}
                        Err(InterpreterError::Break { value, .. }) => {
                            return Ok(value.unwrap_or(Value::Nil));
                        }
                        Err(InterpreterError::Continue { .. }) => continue,
                        Err(e) => return Err(e),
                    }
                }
            }
            Expr::Break { value, span } => {
                let v = match value {
                    Some(e) => Some(self.eval_expr(env, e)?),
                    None => None,
                };
                Err(InterpreterError::Break {
                    value: v,
                    span: Some(*span),
                })
            }
            Expr::Continue { span } => {
                Err(InterpreterError::Continue { span: Some(*span) })
            }

            // --- exceptions ---
            Expr::Raise { expr, span } => {
                let val = self.eval_expr(env, expr)?;
                Err(InterpreterError::UserException {
                    value: val,
                    span: Some(*span),
                })
            }

            Expr::Try {
                body,
                rescues,
                ensure,
                span,
            } => {
                let result = self.eval_expr(env, body);
                let result = match result {
                    Err(ref e) if e.is_user_exception() => {
                        let exc_val = e.as_exception_value().cloned().unwrap();
                        let mut handled: Option<Result<Value, InterpreterError>> = None;
                        for rescue in rescues {
                            let type_matches = match &rescue.type_name {
                                None => true,
                                Some(tn) => match &exc_val {
                                    Value::Variant { name, .. } | Value::Struct { name, .. } => {
                                        name.as_str() == tn.as_str()
                                    }
                                    _ => false,
                                },
                            };
                            if type_matches {
                                let rescue_env = Environment::child(env);
                                if let Some(b) = &rescue.bind {
                                    rescue_env.borrow_mut().define(b.clone(), exc_val.clone());
                                }
                                handled = Some(self.eval_expr(&rescue_env, &rescue.body));
                                break;
                            }
                        }
                        match handled {
                            Some(r) => r,
                            None => Err(InterpreterError::UserException {
                                value: exc_val,
                                span: Some(*span),
                            }),
                        }
                    }
                    other => other,
                };
                // `ensure` is always evaluated; its value is discarded.
                if let Some(ensure_body) = ensure {
                    let _ = self.eval_expr(env, ensure_body);
                }
                result
            }

            // --- control transfers ---
            Expr::Return { value, span } => {
                let v = match value {
                    Some(e) => self.eval_expr(env, e)?,
                    None => Value::Nil,
                };
                Err(InterpreterError::Return {
                    value: v,
                    span: Some(*span),
                })
            }
            Expr::Reply { value, span } => {
                let v = self.eval_expr(env, value)?;
                Err(InterpreterError::Reply {
                    value: v,
                    span: Some(*span),
                })
            }
            Expr::Yield { .. } => {
                // Drain all pending `!` messages from live actors' mailboxes.
                // Uses async drain (coroutine-based) so handlers can `await`.
                self.drain_mailboxes_async()?;
                Ok(Value::Nil)
            }
            Expr::Await { expr, span } => {
                // Zig-style colorless async: evaluate the expression to get a
                // Task, then suspend the current coroutine until the task is
                // ready. If not in a coroutine, poll synchronously (blocking).
                let task_val = self.eval_expr(env, expr)?;
                let task_ref = match &task_val {
                    Value::Task(t) => t.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Task",
                            got: other.type_name(),
                            op: "await".into(),
                            span: Some(*span),
                        })
                    }
                };
                if crate::runtime::scheduler::in_coroutine() {
                    // Inside a coroutine: suspend and let scheduler poll.
                    let v = crate::runtime::scheduler::await_task(task_ref);
                    Ok(v)
                } else {
                    // Not in a coroutine: poll synchronously (busy-wait).
                    // This is the fallback for top-level await.
                    loop {
                        let ready = {
                            let t = task_ref.borrow();
                            match &*t {
                                crate::value::TaskState::Ready(v) => Some(v.clone()),
                                crate::value::TaskState::Consumed => Some(Value::Nil),
                                crate::value::TaskState::Pending(f) => match f() {
                                    crate::value::TaskPoll::Ready(v) => Some(v),
                                    crate::value::TaskPoll::Pending => None,
                                },
                            }
                        };
                        if let Some(v) = ready {
                            *task_ref.borrow_mut() = crate::value::TaskState::Consumed;
                            return Ok(v);
                        }
                        // Spin briefly to avoid 100% CPU.
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }

            Expr::SharedExpr { expr, span: _ } => {
                // `shared expr` — create a SharedRef wrapping the evaluated value.
                let v = self.eval_expr(env, expr)?;
                let cell = crate::value::SharedCell { value: v, version: 0 };
                let sref: SharedRef = Rc::new(RefCell::new(cell));
                Ok(Value::Shared(sref))
            }

            // --- actors ---
            Expr::Spawn { name, args, span } => {
                let ad = self.actors.get(name).cloned().ok_or_else(|| {
                    InterpreterError::NameError {
                        name: name.clone(),
                        span: Some(*span),
                    }
                })?;
                // The actor's environment is a child of the caller's environment,
                // giving it access to module-level functions and imports (like
                // `socket`, `parse_request`) via the scope chain. Actor isolation
                // is provided by `state` decls (own mutable bindings) and message
                // passing — the parent chain is effectively read-only to the actor.
                let actor_env = Environment::child(env);
                // Evaluate constructor args in the caller's environment.
                let mut arg_vals = Vec::with_capacity(args.len());
                for a in args {
                    arg_vals.push(self.eval_expr(env, a)?);
                }
                // Process the actor body: `state` decls initialize bindings,
                // `on` clauses register handlers, `fn` defs are available too.
                for stmt in &ad.body {
                    match stmt {
                        Stmt::StateDecl { name, value, .. } => {
                            let v = self.eval_expr(&actor_env, value)?;
                            actor_env.borrow_mut().define(name.clone(), v);
                        }
                        Stmt::OnClause(_) => {
                            // Handler registered below (needs ActorRef).
                            // Do NOT define the name in env — it would shadow
                            // global builtins like `get`/`count` that handlers
                            // may need to call.
                        }
                        Stmt::FuncDef(fd) => {
                            let closure = Closure {
                                params: fd.params.clone(),
                                body: (*fd.body).clone(),
                                env: actor_env.clone(),
                                name: Some(fd.name.clone()),
                            };
                            actor_env
                                .borrow_mut()
                                .define(fd.name.clone(), Value::Func(Rc::new(closure)));
                        }
                        _ => {
                            // Ignore other statements in actor body.
                        }
                    }
                }
                let actor_ref: ActorRef = Rc::new(RefCell::new(ActorInstance::new(actor_env)));
                // Now register handlers with access to the ActorRef (self).
                for stmt in &ad.body {
                    if let Stmt::OnClause(oc) = stmt {
                        actor_ref
                            .borrow_mut()
                            .handlers
                            .insert(oc.name.clone(), oc.clone());
                    }
                }
                // Bind `self` so handlers can refer to their own actor.
                actor_ref
                    .borrow()
                    .env
                    .borrow_mut()
                    .define("self", Value::Actor(actor_ref.clone()));
                self.live_actors.push(actor_ref.clone());
                Ok(Value::Actor(actor_ref))
            }

            Expr::ActorSend { actor, msg, span } => {
                // `actor ! msg` — fire-and-forget: enqueue and return nil.
                let actor_val = self.eval_expr(env, actor)?;
                let actor_ref = match &actor_val {
                    Value::Actor(ar) => ar.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Actor",
                            got: other.type_name(),
                            op: "send (`!`)".into(),
                            span: Some(*span),
                        })
                    }
                };
                let msg_val = self.eval_message(env, msg)?;
                actor_ref.borrow_mut().mailbox.push_back(Envelope {
                    msg: msg_val,
                    reply_slot: None,
                });
                Ok(Value::Nil)
            }

            Expr::ActorRequest { actor, msg, span } => {
                // `actor ? msg` — synchronous request/reply.
                let actor_val = self.eval_expr(env, actor)?;
                let actor_ref = match &actor_val {
                    Value::Actor(ar) => ar.clone(),
                    other => {
                        return Err(InterpreterError::TypeError {
                            expected: "Actor",
                            got: other.type_name(),
                            op: "request (`?`)".into(),
                            span: Some(*span),
                        })
                    }
                };
                let msg_val = self.eval_message(env, msg)?;
                // Dispatch directly to the matching handler (synchronous).
                self.dispatch_request(&actor_ref, &msg_val, *span)
            }

            // --- Phase 3: transactional memory ---
            Expr::Transact { body, span } => {
                self.eval_transact(env, body, *span)
            }
            Expr::Retry { span } => {
                // Signal the enclosing `transact` to restart. If there is no
                // enclosing transaction, this escapes as a runtime error.
                Err(InterpreterError::Retry { span: Some(*span) })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers for loops / compound assignment
    // -----------------------------------------------------------------------

    /// Apply a binary operator to two values (used by compound assignment).
    /// Mirrors the `Expr::BinOp` evaluation but without short-circuit (And/Or
    /// are not valid compound-assign ops).
    fn apply_binop(
        &mut self,
        op: BinOp,
        l: &Value,
        r: &Value,
    ) -> Result<Value, InterpreterError> {
        match op {
            BinOp::Add => {
                if let (Value::Str(a), Value::Str(b)) = (l, r) {
                    Ok(Value::str(format!("{}{}", a, b)))
                } else {
                    ops::add(l, r)
                }
            }
            BinOp::Sub => ops::sub(l, r),
            BinOp::Mul => ops::mul(l, r),
            BinOp::Div => ops::div(l, r),
            BinOp::Mod => ops::modulo(l, r),
            _ => Err(InterpreterError::RuntimeError {
                msg: format!("unsupported compound-assign operator: {}", op.as_str()),
                span: None,
            }),
        }
    }

    /// Convert an iterable value into a `Vec<Value>` for `for ... in`.
    /// - Vec: elements in order
    /// - Set: elements in arbitrary order
    /// - Map: `[key, value]` pairs (as `Value::Vec` of length 2)
    /// - Str: characters (as 1-char `Value::Str`)
    fn iter_to_vec(&self, v: &Value) -> Result<Vec<Value>, InterpreterError> {
        match v {
            Value::Vec(v) => Ok(v.iter().cloned().collect()),
            Value::Set(s) => Ok(s.iter().cloned().collect()),
            Value::Map(m) => Ok(m
                .iter()
                .map(|(k, val)| Value::vec(vec![k.clone(), val.clone()]))
                .collect()),
            Value::Str(s) => Ok(s
                .chars()
                .map(|c| Value::str(c.to_string()))
                .collect()),
            _ => Err(InterpreterError::TypeError {
                expected: "Vec, Set, Map, or Str (iterable)",
                got: v.type_name(),
                op: "for..in".into(),
                span: None,
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Higher-order collection ops (Phase 3.5b)
    // -----------------------------------------------------------------------

    /// `map(coll, fn)` — apply `fn` to each element, return a new Vec.
    fn ho_map(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        let (coll, f) = two_args(args, "map", span)?;
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        let mut out = Vec::with_capacity(items.len());
        for item in &items {
            out.push(self.call_function(&f, vec![item.clone()], span)?);
        }
        Ok(Value::vec(out))
    }

    /// `filter(coll, fn)` — keep elements where `fn` returns truthy.
    fn ho_filter(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        let (coll, f) = two_args(args, "filter", span)?;
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        let mut out = Vec::new();
        for item in &items {
            let keep = self.call_function(&f, vec![item.clone()], span)?;
            if keep.is_truthy() {
                out.push(item.clone());
            }
        }
        Ok(Value::vec(out))
    }

    /// `fold(coll, init, fn)` — left fold with initial value.
    /// `fn(acc, elem) -> new_acc`
    fn ho_fold(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        if args.len() != 3 {
            return Err(InterpreterError::ArityError {
                expected: 3,
                got: args.len(),
                callee: "fold".into(),
                span: Some(span),
            });
        }
        let coll = args[0].clone();
        let mut acc = args[1].clone();
        let f = args[2].clone();
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        for item in &items {
            acc = self.call_function(&f, vec![acc, item.clone()], span)?;
        }
        Ok(acc)
    }

    /// `reduce(coll, fn)` — left fold without initial value.
    /// `fn(acc, elem) -> new_acc`. Collection must be non-empty.
    fn ho_reduce(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        let (coll, f) = two_args(args, "reduce", span)?;
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        if items.is_empty() {
            return Err(InterpreterError::RuntimeError {
                msg: "reduce of empty collection".into(),
                span: Some(span),
            });
        }
        let mut acc = items[0].clone();
        for item in &items[1..] {
            acc = self.call_function(&f, vec![acc, item.clone()], span)?;
        }
        Ok(acc)
    }

    /// `find(coll, fn)` — first element where `fn` returns truthy, else Nil.
    fn ho_find(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        let (coll, f) = two_args(args, "find", span)?;
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        for item in &items {
            let found = self.call_function(&f, vec![item.clone()], span)?;
            if found.is_truthy() {
                return Ok(item.clone());
            }
        }
        Ok(Value::Nil)
    }

    /// `each(coll, fn)` — call `fn` on each element for side effects, return Nil.
    fn ho_each(&mut self, args: &[Value], span: Span) -> Result<Value, InterpreterError> {
        let (coll, f) = two_args(args, "each", span)?;
        let items = self.iter_to_vec(&coll).map_err(|e| e.with_span(span))?;
        for item in &items {
            self.call_function(&f, vec![item.clone()], span)?;
        }
        Ok(Value::Nil)
    }

    // -----------------------------------------------------------------------
    // Transaction (Phase 3)
    // -----------------------------------------------------------------------

    /// Maximum number of retry attempts before a transaction gives up.
    const MAX_RETRIES: usize = 64;

    /// Execute a `transact { body }` block with snapshot isolation and
    /// atomic commit/rollback.
    ///
    /// Semantics:
    /// - **Isolation**: reads of `shared` refs check the transaction's
    ///   write-set first, then the committed value (recording the version).
    /// - **Atomicity**: on normal exit, all buffered writes are applied
    ///   atomically (versions bumped). On exception, writes are discarded.
    /// - **Retry**: `retry` inside the body discards the current attempt and
    ///   restarts (up to `MAX_RETRIES`).
    /// - **Nesting**: a nested `transact` commits its writes into the parent's
    ///   write-set (not to the refs directly). A nested rollback discards only
    ///   the inner writes.
    fn eval_transact(
        &mut self,
        env: &EnvRef,
        body: &Expr,
        span: Span,
    ) -> Result<Value, InterpreterError> {
        for _attempt in 0..Self::MAX_RETRIES {
            let ctx = TransactionContext::new();
            self.txn_stack.push(ctx);

            match self.eval_expr(env, body) {
                Ok(result) => {
                    // Commit: pop the context and merge/apply writes.
                    let inner = self.txn_stack.pop().expect("txn stack imbalance");
                    match self.commit_txn(inner, result) {
                        CommitOutcome::Done(v) => return Ok(v),
                        CommitOutcome::Conflict => continue,
                    }
                }
                Err(InterpreterError::Retry { .. }) => {
                    // Discard this attempt and retry.
                    self.txn_stack.pop();
                    continue;
                }
                // `reply`/`return` inside a transaction are control-flow
                // exits, NOT exceptions: the transaction must commit before
                // the signal propagates. On conflict, retry (the buffered
                // value was computed from a stale snapshot).
                Err(InterpreterError::Reply { value, span }) => {
                    let inner = self.txn_stack.pop().expect("txn stack imbalance");
                    match self.commit_txn(inner, value.clone()) {
                        CommitOutcome::Done(_) => {
                            return Err(InterpreterError::Reply { value, span })
                        }
                        CommitOutcome::Conflict => continue,
                    }
                }
                Err(InterpreterError::Return { value, span }) => {
                    let inner = self.txn_stack.pop().expect("txn stack imbalance");
                    match self.commit_txn(inner, value.clone()) {
                        CommitOutcome::Done(_) => {
                            return Err(InterpreterError::Return { value, span })
                        }
                        CommitOutcome::Conflict => continue,
                    }
                }
                Err(e) => {
                    // Rollback: discard writes, propagate the error.
                    self.txn_stack.pop();
                    return Err(e);
                }
            }
        }
        // Exhausted all retries.
        Err(InterpreterError::RuntimeError {
            msg: format!("transaction exceeded {} retries", Self::MAX_RETRIES),
            span: Some(span),
        })
    }

    /// Commit a popped transaction context.
    ///
    /// - Nested transaction: merge writes/reads into the parent's write-set
    ///   (always succeeds — conflict is checked at the outermost level).
    /// - Outermost transaction: validate reads; on conflict return
    ///   [`CommitOutcome::Conflict`] so the caller retries. On success apply
    ///   all writes atomically (bumping versions).
    fn commit_txn(&mut self, inner: TransactionContext, result: Value) -> CommitOutcome {
        if let Some(parent) = self.txn_stack.last_mut() {
            // Nested commit: merge writes into parent's write-set.
            for (id, (rref, val)) in inner.writes {
                parent.writes.insert(id, (rref, val));
            }
            // Merge reads: keep the earliest version per ref.
            for (id, (rref, ver)) in inner.reads {
                parent.reads.entry(id).or_insert((rref, ver));
            }
            return CommitOutcome::Done(result);
        }
        // Outermost commit: validate reads for conflicts.
        let conflict = inner
            .reads
            .values()
            .any(|(rref, ver)| rref.borrow().version != *ver);
        if conflict {
            // A direct write modified a ref we read — restart.
            return CommitOutcome::Conflict;
        }
        // Apply all writes atomically (bump versions).
        for (_, (rref, val)) in &inner.writes {
            let mut cell = rref.borrow_mut();
            cell.value = val.clone();
            cell.version += 1;
        }
        CommitOutcome::Done(result)
    }

    /// Evaluate a message expression. A bare identifier `Inc` or a call
    /// `Add(5, 3)` where the callee is an undefined identifier becomes a
    /// `Value::Variant` — this lets users write `actor ! Inc` / `actor ! Add(5, 3)`
    /// without pre-declaring the message as an enum.
    fn eval_message(&mut self, env: &EnvRef, msg: &Expr) -> Result<Value, InterpreterError> {
        match msg {
            Expr::Ident(name, _) => {
                // If the name is already bound (e.g. a zero-arity enum variant),
                // use its value; otherwise synthesize a zero-arg Variant.
                match env.borrow().get(name) {
                    Some(v) => Ok(v),
                    None => Ok(Value::Variant {
                        name: Rc::new(name.clone()),
                        args: Rc::new(vec![]),
                    }),
                }
            }
            Expr::Call { callee, args, .. } => {
                // In message position, `Name(args)` is ALWAYS a message
                // variant, never a function call — even if `Name` happens to
                // be bound to a builtin (e.g. `get`). This lets users write
                // `actor ? get()` without colliding with the global `get`
                // builtin. Enum-variant constructors (`Some(42)`) also produce
                // the same Variant, so we synthesize unconditionally.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    let mut arg_vals = Vec::with_capacity(args.len());
                    for a in args {
                        arg_vals.push(self.eval_expr(env, a)?);
                    }
                    return Ok(Value::Variant {
                        name: Rc::new(name.clone()),
                        args: Rc::new(arg_vals),
                    });
                }
                self.eval_expr(env, msg)
            }
            _ => self.eval_expr(env, msg),
        }
    }

    /// Dispatch a message to the matching `on` handler. For `?` (request),
    /// wait for `reply` and return its value.
    fn dispatch_request(
        &mut self,
        actor_ref: &ActorRef,
        msg: &Value,
        span: Span,
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
                })
            }
        };
        let oc = {
            // Clone the handler out to avoid borrowing actor_ref during eval.
            actor_ref
                .borrow()
                .handlers
                .get(&handler_name)
                .cloned()
                .ok_or_else(|| InterpreterError::RuntimeError {
                    msg: format!(
                        "actor has no `on {}` handler for message `{}`",
                        handler_name, handler_name
                    ),
                    span: Some(span),
                })?
        };
        // Bind message args to handler params in a child of the actor's env.
        let call_env = Environment::child(&actor_ref.borrow().env);
        if let Value::Variant { args, .. } = msg {
            if oc.params.len() != args.len() {
                return Err(InterpreterError::ArityError {
                    expected: oc.params.len(),
                    got: args.len(),
                    callee: format!("on {}", handler_name),
                    span: Some(span),
                });
            }
            for (param, arg) in oc.params.iter().zip(args.iter()) {
                call_env.borrow_mut().define(param.name.clone(), arg.clone());
            }
        }
        // Evaluate the handler body. `reply expr` unwinds via Reply signal.
        match self.eval_expr(&call_env, &oc.body) {
            Ok(_) => {
                // Handler fell through without `reply` — return nil.
                Ok(Value::Nil)
            }
            Err(InterpreterError::Reply { value, .. }) => Ok(value),
            Err(e) => Err(e),
        }
    }

    /// Process all pending `!` messages in every live actor's mailbox.
    /// Called after the main program finishes.
    fn drain_mailboxes(&mut self) -> Result<(), InterpreterError> {
        // Iterate by index so we can mutably borrow `self` during dispatch.
        let mut i = 0;
        while i < self.live_actors.len() {
            let actor_ref = self.live_actors[i].clone();
            // Drain all currently-queued messages for this actor.
            // New messages queued during processing will also be drained.
            loop {
                let env_opt = actor_ref.borrow_mut().mailbox.pop_front();
                let env = match env_opt {
                    Some(e) => e,
                    None => break,
                };
                // `!` messages have no reply slot; discard any reply.
                match self.dispatch_request(&actor_ref, &env.msg, Span::dummy()) {
                    Ok(_) => {}
                    // Surface errors from fire-and-forget handlers as runtime errors.
                    Err(e) => return Err(e),
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// Like `drain_mailboxes`, but each handler runs in its own coroutine,
    /// enabling `await` inside handlers. Used by the HTTP server event loop.
    ///
    /// # Safety
    /// This method uses raw pointers to share `self` across coroutines.
    /// It is safe because: (1) single-threaded, (2) corosensei's `suspend`
    /// returns control to this function, which never accesses `self` while
    /// a coroutine is running, (3) coroutines are resumed one at a time.
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
        let self_ptr: *mut Interpreter = self;
        for (actor_ref, msg) in pending {
            // Clone the handler + env out so the coroutine closure is 'static.
            let handler_name = match &msg {
                Value::Variant { name, .. } => name.as_str().to_string(),
                _ => continue, // skip non-variant messages
            };
            let oc = match actor_ref.borrow().handlers.get(&handler_name) {
                Some(h) => h.clone(),
                None => continue, // no handler — skip
            };
            let call_env = Environment::child(&actor_ref.borrow().env);
            if let Value::Variant { args, .. } = &msg {
                if oc.params.len() != args.len() {
                    return Err(InterpreterError::ArityError {
                        expected: oc.params.len(),
                        got: args.len(),
                        callee: format!("on {}", handler_name),
                        span: Some(Span::dummy()),
                    });
                }
                for (param, arg) in oc.params.iter().zip(args.iter()) {
                    call_env.borrow_mut().define(param.name.clone(), arg.clone());
                }
            }

            // Spawn coroutine: the closure captures the raw pointer to self.
            // SAFETY: self_ptr is valid for the duration of run_until_complete
            // because `self` outlives the scheduler. The coroutine only
            // accesses the interpreter while running (not while suspended),
            // and the scheduler only runs one coroutine at a time.
            self.scheduler.spawn_handler(move || {
                // SAFETY: the coroutine runs inside drain_mailboxes_async,
                // which holds a unique borrow on self. The scheduler never
                // accesses self_ptr while this coroutine is suspended.
                let interp: &mut Interpreter = unsafe { &mut *self_ptr };
                match interp.eval_expr(&call_env, &oc.body) {
                    Ok(_) => Ok(Value::Nil),
                    Err(InterpreterError::Reply { value, .. }) => Ok(value),
                    Err(e) => Err(e),
                }
            });
        }

        // Run the scheduler until all coroutines complete or park on I/O.
        let results = self.scheduler.run_until_complete();
        for r in results {
            r?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Function call
    // -----------------------------------------------------------------------

    fn call_function(
        &mut self,
        func: &Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, InterpreterError> {
        match func {
            Value::Func(closure) => {
                if closure.params.len() != args.len() {
                    return Err(InterpreterError::ArityError {
                        expected: closure.params.len(),
                        got: args.len(),
                        callee: closure.name.clone().unwrap_or_else(|| "<lambda>".into()),
                        span: Some(span),
                    });
                }
                let call_env = Environment::child(&closure.env);
                for (param, arg) in closure.params.iter().zip(args.into_iter()) {
                    call_env.borrow_mut().define(param.name.clone(), arg);
                }
                match self.eval_expr(&call_env, &closure.body) {
                    Ok(v) => Ok(v),
                    // `return` unwinds to here — the enclosing function call.
                    Err(InterpreterError::Return { value, .. }) => Ok(value),
                    Err(e) => Err(e),
                }
            }
            Value::Native(nf) => {
                // Higher-order builtins need to call user closures, which
                // requires interpreter access. Route by name instead of
                // invoking `func` directly.
                match nf.name {
                    "map" => self.ho_map(&args, span),
                    "filter" => self.ho_filter(&args, span),
                    "fold" => self.ho_fold(&args, span),
                    "reduce" => self.ho_reduce(&args, span),
                    "find" => self.ho_find(&args, span),
                    "each" => self.ho_each(&args, span),
                    _ => (nf.func)(&args).map_err(|e| e.with_span(span)),
                }
            }
            _ => Err(InterpreterError::TypeError {
                expected: "callable",
                got: func.type_name(),
                op: "call".into(),
                span: Some(span),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Assignment (lvalue dispatch)
    // -----------------------------------------------------------------------

    fn assign_to(
        &mut self,
        env: &EnvRef,
        target: &Expr,
        value: Value,
    ) -> Result<(), InterpreterError> {
        match target {
            Expr::Ident(name, span) => {
                // If the binding is a SharedRef, write through it (buffer in
                // the active transaction, or update directly + bump version).
                let bound = env.borrow().get(name);
                if let Some(Value::Shared(sref)) = &bound {
                    let id = TransactionContext::ref_id(sref);
                    if let Some(ctx) = self.txn_stack.last_mut() {
                        // Inside a transaction: buffer the write.
                        ctx.writes.insert(id, (sref.clone(), value));
                    } else {
                        // Outside: direct write + version bump.
                        let mut cell = sref.borrow_mut();
                        cell.value = value;
                        cell.version += 1;
                    }
                    Ok(())
                } else if env.borrow_mut().assign(name, value) {
                    Ok(())
                } else {
                    Err(InterpreterError::NameError {
                        name: name.clone(),
                        span: Some(*span),
                    })
                }
            }
            Expr::Field {
                target: obj,
                name,
                span,
            } => {
                let obj_val = self.eval_expr(env, obj)?;
                let new_val = match &obj_val {
                    Value::Struct {
                        name: sname,
                        fields,
                    } => {
                        let mut new_fields = (**fields).clone();
                        new_fields.insert(name.clone(), value);
                        Value::Struct {
                            name: sname.clone(),
                            fields: Rc::new(new_fields),
                        }
                    }
                    Value::Map(m) => {
                        let key = Value::str(name);
                        Value::Map(m.update(key, value))
                    }
                    _ => {
                        return Err(InterpreterError::TypeError {
                            expected: "Struct or Map",
                            got: obj_val.type_name(),
                            op: format!("field assignment `.{}", name),
                            span: Some(*span),
                        })
                    }
                };
                self.assign_to(env, obj, new_val)
            }
            Expr::Index {
                target: coll,
                index,
                span,
            } => {
                let coll_val = self.eval_expr(env, coll)?;
                let idx_val = self.eval_expr(env, index)?;
                let new_val = match &coll_val {
                    Value::Vec(v) => {
                        let i = match &idx_val {
                            Value::Int(n) => n.to_usize(),
                            _ => {
                                return Err(InterpreterError::TypeError {
                                    expected: "Int",
                                    got: idx_val.type_name(),
                                    op: "index assignment".into(),
                                    span: Some(*span),
                                })
                            }
                        };
                        match i {
                            Some(i) if i < v.len() => {
                                let mut new_v = v.clone();
                                new_v[i] = value;
                                Value::Vec(new_v)
                            }
                            _ => {
                                return Err(InterpreterError::IndexError {
                                    msg: format!("index out of bounds: {}", idx_val),
                                    span: Some(*span),
                                })
                            }
                        }
                    }
                    Value::Map(m) => Value::Map(m.update(idx_val, value)),
                    _ => {
                        return Err(InterpreterError::TypeError {
                            expected: "Vec or Map",
                            got: coll_val.type_name(),
                            op: "index assignment".into(),
                            span: Some(*span),
                        })
                    }
                };
                self.assign_to(env, coll, new_val)
            }
            _ => Err(InterpreterError::RuntimeError {
                msg: "invalid assignment target".into(),
                span: Some(target.span()),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Pattern matching
    // -----------------------------------------------------------------------

    /// Try to match `value` against `pattern`. On success, bindings are
    /// inserted into `env`. Returns `Ok(true)` if the pattern matched.
    pub fn match_pattern(
        &mut self,
        pattern: &Pattern,
        value: &Value,
        env: &EnvRef,
    ) -> Result<bool, InterpreterError> {
        match pattern {
            Pattern::Wildcard(_) => Ok(true),

            Pattern::Lit(lit, _) => {
                let lit_val = match lit {
                    LitPattern::Int(n) => Value::Int(n.clone()),
                    LitPattern::Decimal(d) => Value::Decimal(d.clone()),
                    LitPattern::Bool(b) => Value::Bool(*b),
                    LitPattern::Nil => Value::Nil,
                    LitPattern::Str(parts) => {
                        let mut result = String::new();
                        for part in parts {
                            match part {
                                StrPart::Literal(s) => result.push_str(s),
                                StrPart::Expr(e) => {
                                    let v = self.eval_expr(env, e)?;
                                    result.push_str(&format!("{}", v));
                                }
                            }
                        }
                        Value::str(result)
                    }
                };
                Ok(&lit_val == value)
            }

            Pattern::Bind(name, _) => {
                env.borrow_mut().define(name.clone(), value.clone());
                Ok(true)
            }

            Pattern::Variant { name, args, .. } => match value {
                Value::Variant {
                    name: vn,
                    args: va,
                } => {
                    if vn.as_str() != name.as_str() || args.len() != va.len() {
                        return Ok(false);
                    }
                    for (pat, val) in args.iter().zip(va.iter()) {
                        if !self.match_pattern(pat, val, env)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },

            Pattern::Struct { fields, rest, .. } => match value {
                Value::Struct { fields: vf, .. } => {
                    for (fname, fpat) in fields {
                        match vf.get(fname) {
                            Some(fval) => {
                                if !self.match_pattern(fpat, fval, env)? {
                                    return Ok(false);
                                }
                            }
                            None => return Ok(false),
                        }
                    }
                    let _ = rest; // `..` allows partial match — no extra check needed.
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
                        if !self.match_pattern(pat, &v[i], env)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },

            Pattern::Or(pats, _) => {
                for pat in pats {
                    // Each alternative gets a fresh sub-environment so that
                    // failed matches don't leak bindings.
                    let sub_env = Environment::child(env);
                    if self.match_pattern(pat, value, &sub_env)? {
                        // Transfer successful bindings into the real env.
                        let bindings = sub_env.borrow().bindings_clone();
                        for (k, v) in bindings {
                            env.borrow_mut().define(k, v);
                        }
                        return Ok(true);
                    }
                }
                Ok(false)
            }

            Pattern::Guard(pat, guard, _) => {
                if self.match_pattern(pat, value, env)? {
                    let g = self.eval_expr(env, guard)?;
                    Ok(g.is_truthy())
                } else {
                    Ok(false)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point for external callers
// ---------------------------------------------------------------------------

/// Convenience: parse and evaluate `source` in a fresh interpreter.
pub fn run(source: &str) -> Result<(), InterpreterError> {
    let mut interp = Interpreter::new();
    interp.run(source)
}
