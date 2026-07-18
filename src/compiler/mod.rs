//! AST → bytecode compiler for the 1y VM.
//!
//! Walks the AST and emits [`Chunk`]s of bytecode. Uses **compile-time slot
//! indexing** (Lua/clox style): each local variable is assigned a fixed slot
//! relative to the frame base, and upvalues are resolved at compile time so
//! the VM can capture them with O(1) access.
//!
//! # Scope model
//!
//! - `FuncCtx::scope_depth` starts at 0 for the function body.
//! - `begin_scope` increments depth; `end_scope` decrements it and pops locals.
//! - At script level (depth 0), `let` creates a **global**; inside a function,
//!   `let` creates a **local**.
//!
//! # Upvalue resolution
//!
//! When an identifier is not found in the current function's locals, the
//! compiler searches the enclosing function's locals (marking the found local
//! as `is_captured`), or recursively the enclosing's upvalues. Each level
//! adds an upvalue entry so the chain is properly linked at runtime.

use crate::ast::*;
use crate::interpreter::error::InterpreterError;
use crate::value::Value;
use crate::vm::chunk::{write_u8, write_u16, Chunk, OpCode};
use std::collections::HashMap;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compile a parsed [`Program`] into a top-level [`Chunk`].
///
/// This is the simple entry point: it starts from an empty type table, so
/// `eval`-ed code that references `enum` variants / `type` structs defined
/// outside the eval string will fail to compile them as constructors.
/// Use [`compile_program_with_types`] when the caller maintains a persistent
/// type table (e.g. the VM, which needs `eval` to see outer `enum`/`type`
/// definitions).
pub fn compile_program(program: &Program) -> Result<Chunk, InterpreterError> {
    let mut variants = HashMap::new();
    let mut structs = HashMap::new();
    compile_program_with_types(program, &mut variants, &mut structs)
}

/// Like [`compile_program`], but seeds the compiler with the caller's
/// already-known variant / struct names, and writes back any newly
/// registered types encountered while compiling `program`.
///
/// This lets `eval(src)` recognize `enum` variants and `type` structs
/// defined at the outer scope, so `eval("EvError.Bad(\"x\")")` works
/// after `enum EvError { Bad(String) }` has been defined.
pub fn compile_program_with_types(
    program: &Program,
    variants: &mut HashMap<String, usize>,
    structs: &mut HashMap<String, ()>,
) -> Result<Chunk, InterpreterError> {
    let mut compiler = Compiler::new(FunctionType::Script);
    // Seed with the caller's known types so the compiled code can
    // reference outer-scope enum variants / struct constructors.
    compiler.variants = std::mem::take(variants);
    compiler.structs = std::mem::take(structs);

    let n = program.stmts.len();
    if n == 0 {
        // Empty program: push Nil so the closing `Return` has something
        // to pop (otherwise the VM stack-underflows).
        compiler.emit_op(OpCode::Nil);
    }
    for (i, stmt) in program.stmts.iter().enumerate() {
        if i + 1 == n {
            // Last top-level statement: if it's an expression, keep its value
            // as the program result.
            match stmt {
                Stmt::Expr(e) => {
                    compiler.compile_expr(e)?;
                }
                Stmt::Semi(e) => {
                    compiler.compile_expr(e)?;
                    compiler.emit_op(OpCode::Pop);
                    compiler.emit_op(OpCode::Nil);
                }
                other => {
                    compiler.compile_stmt(other)?;
                    compiler.emit_op(OpCode::Nil);
                }
            }
        } else {
            compiler.compile_stmt(stmt)?;
        }
    }
    compiler.emit_op(OpCode::Return);

    // Sync back the (possibly extended) type tables so the caller sees
    // any `enum` / `type` definitions introduced by this program.
    *variants = std::mem::take(&mut compiler.variants);
    *structs = std::mem::take(&mut compiler.structs);

    Ok(compiler.finish())
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum FunctionType {
    Script,
    Function,
}

struct Local {
    name: String,
    depth: usize,
    is_captured: bool,
}

#[derive(Clone, Copy)]
struct UpvalueInfo {
    is_local: bool,
    index: usize,
}

struct FuncCtx {
    chunk: Chunk,
    function_type: FunctionType,
    scope_depth: usize,
    locals: Vec<Local>,
    upvalues: Vec<UpvalueInfo>,
}

impl FuncCtx {
    fn new(function_type: FunctionType, name: Option<String>) -> Self {
        FuncCtx {
            chunk: Chunk::new(name, 0),
            function_type,
            scope_depth: 0,
            locals: Vec::new(),
            upvalues: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

pub struct Compiler {
    func_stack: Vec<FuncCtx>,
    /// Enum variant constructors: `variant_name → arity`. Used to compile
    /// `Name(args)` calls as `ConstructVariant` instead of `LoadGlobal; Call`.
    /// Zero-arity variants are also bound as globals (Value::Variant).
    variants: HashMap<String, usize>,
    /// Struct type names registered via `type Name = { ... }`. Used to
    /// compile `Name({ ... })` calls as `ConstructStruct`. Field types are
    /// not enforced (mirrors the tree-walker, which is dynamically typed).
    structs: HashMap<String, ()>,
}

impl Compiler {
    fn new(function_type: FunctionType) -> Self {
        let ctx = FuncCtx::new(function_type, Some("<script>".into()));
        Compiler {
            func_stack: vec![ctx],
            variants: HashMap::new(),
            structs: HashMap::new(),
        }
    }

    fn finish(self) -> Chunk {
        // Extract the top-level chunk. Unwrap Rc if needed.
        let mut func_stack = self.func_stack;
        let ctx = func_stack.pop().expect("func_stack should not be empty");
        ctx.chunk
    }

    // --- context access ---

    fn current(&self) -> &FuncCtx {
        self.func_stack.last().expect("func_stack empty")
    }

    fn current_mut(&mut self) -> &mut FuncCtx {
        self.func_stack.last_mut().expect("func_stack empty")
    }

    fn current_code_len(&self) -> usize {
        self.current().chunk.code.len()
    }

    // --- emission ---

    fn emit_op(&mut self, op: OpCode) {
        let span = crate::ast::Span::dummy();
        let ctx = self.current_mut();
        ctx.chunk.code.push(op as u8);
        ctx.chunk.spans.push(span);
    }

    fn emit_u8(&mut self, v: u8) {
        let ctx = self.current_mut();
        write_u8(&mut ctx.chunk.code, v);
        ctx.chunk.spans.push(crate::ast::Span::dummy());
    }

    fn emit_u16(&mut self, v: u16) {
        let ctx = self.current_mut();
        write_u16(&mut ctx.chunk.code, v);
        ctx.chunk.spans.push(crate::ast::Span::dummy());
        ctx.chunk.spans.push(crate::ast::Span::dummy());
    }

    fn emit_const(&mut self, v: Value) -> u8 {
        let ctx = self.current_mut();
        // Reuse existing constant if present.
        for (i, existing) in ctx.chunk.consts.iter().enumerate() {
            if existing == &v {
                return i as u8;
            }
        }
        let idx = ctx.chunk.consts.len();
        if idx > 255 {
            panic!("too many constants in one chunk");
        }
        ctx.chunk.consts.push(v);
        idx as u8
    }

    fn emit_jump(&mut self, op: OpCode) -> usize {
        self.emit_op(op);
        let offset = self.current_code_len();
        self.emit_u16(0); // placeholder
        offset
    }

    fn patch_jump(&mut self, offset: usize) {
        let target = self.current_code_len() as u16;
        self.patch_u16(offset, target);
    }

    /// Patch a u16 operand at `offset` to `value`.
    fn patch_u16(&mut self, offset: usize, value: u16) {
        let ctx = self.current_mut();
        ctx.chunk.code[offset] = (value & 0xff) as u8;
        ctx.chunk.code[offset + 1] = (value >> 8) as u8;
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_op(OpCode::Loop);
        self.emit_u16(loop_start as u16);
    }

    fn add_pattern(&mut self, pattern: &Pattern) -> u8 {
        let ctx = self.current_mut();
        // Reuse if identical pattern already stored.
        let rc = Rc::new(pattern.clone());
        for (i, existing) in ctx.chunk.patterns.iter().enumerate() {
            if Rc::ptr_eq(existing, &rc) {
                return i as u8;
            }
        }
        let idx = ctx.chunk.patterns.len();
        if idx > 255 {
            panic!("too many patterns in one chunk");
        }
        ctx.chunk.patterns.push(rc);
        idx as u8
    }

    // --- scope management ---

    fn begin_scope(&mut self) {
        self.current_mut().scope_depth += 1;
    }

    /// End the current scope. Returns the number of locals popped.
    fn end_scope(&mut self) -> usize {
        let depth = self.current().scope_depth;
        let mut n = 0;
        while let Some(local) = self.current().locals.last() {
            if local.depth == depth {
                n += 1;
                self.current_mut().locals.pop();
            } else {
                break;
            }
        }
        self.current_mut().scope_depth -= 1;
        n
    }

    /// Declare a local variable at the current scope depth.
    /// Returns its slot index.
    fn declare_local(&mut self, name: &str) -> usize {
        let depth = self.current().scope_depth;
        let ctx = self.current_mut();
        let slot = ctx.locals.len();
        ctx.locals.push(Local {
            name: name.into(),
            depth,
            is_captured: false,
        });
        slot
    }

    fn pop_local(&mut self) {
        self.current_mut().locals.pop();
    }

    fn current_local_count(&self) -> usize {
        self.current().locals.len()
    }

    // --- variable resolution ---

    /// Resolve `name` in the current function's locals. Returns slot index.
    fn resolve_local(&self, name: &str) -> Option<usize> {
        let func = self.current();
        for (i, local) in func.locals.iter().enumerate().rev() {
            if local.name == name && local.depth <= func.scope_depth {
                return Some(i);
            }
        }
        None
    }

    /// Resolve `name` as an upvalue of the current function. Adds upvalue
    /// entries to intermediate functions as needed. Returns upvalue index.
    fn resolve_upvalue(&mut self, name: &str) -> Option<usize> {
        let current_idx = self.func_stack.len() - 1;
        if current_idx == 0 {
            return None;
        }
        self.resolve_upvalue_at(current_idx, name)
    }

    fn resolve_upvalue_at(&mut self, func_idx: usize, name: &str) -> Option<usize> {
        if func_idx == 0 {
            return None;
        }
        let enclosing_idx = func_idx - 1;

        // Try enclosing's locals.
        let enclosing_depth = self.func_stack[enclosing_idx].scope_depth;
        let local_slot = self.func_stack[enclosing_idx]
            .locals
            .iter()
            .rposition(|l| l.name == name && l.depth <= enclosing_depth);

        if let Some(slot) = local_slot {
            // Mark as captured.
            self.func_stack[enclosing_idx].locals[slot].is_captured = true;
            return Some(self.add_upvalue(func_idx, true, slot));
        }

        // Recurse into enclosing's upvalues.
        if enclosing_idx > 0 {
            if let Some(enclosing_uv) = self.resolve_upvalue_at(enclosing_idx, name) {
                return Some(self.add_upvalue(func_idx, false, enclosing_uv));
            }
        }
        None
    }

    fn add_upvalue(&mut self, func_idx: usize, is_local: bool, index: usize) -> usize {
        // Check if upvalue already exists.
        for (i, uv) in self.func_stack[func_idx].upvalues.iter().enumerate() {
            if uv.is_local == is_local && uv.index == index {
                return i;
            }
        }
        let uv_idx = self.func_stack[func_idx].upvalues.len();
        self.func_stack[func_idx].upvalues.push(UpvalueInfo { is_local, index });
        self.func_stack[func_idx].chunk.upvalue_count += 1;
        uv_idx
    }

    /// Resolve a variable name and emit a load instruction.
    fn emit_load(&mut self, name: &str) {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(OpCode::LoadLocal);
            self.emit_u8(slot as u8);
        } else if let Some(idx) = self.resolve_upvalue(name) {
            self.emit_op(OpCode::LoadUpvalue);
            self.emit_u8(idx as u8);
        } else {
            let const_idx = self.emit_const(Value::str(name));
            self.emit_op(OpCode::LoadGlobal);
            self.emit_u8(const_idx);
        }
    }

    /// Like `emit_load` but emits a *Ref load (LoadLocalRef / LoadUpvalueRef
    /// / LoadGlobalRef) that does NOT auto-deref SharedRef at runtime. Used
    /// for bare-identifier function call arguments so that a SharedRef
    /// parameter is passed by reference, mirroring the tree-walker's
    /// call-site fix for SharedRef write-through.
    fn emit_load_ref(&mut self, name: &str) {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(OpCode::LoadLocalRef);
            self.emit_u8(slot as u8);
        } else if let Some(idx) = self.resolve_upvalue(name) {
            self.emit_op(OpCode::LoadUpvalueRef);
            self.emit_u8(idx as u8);
        } else {
            let const_idx = self.emit_const(Value::str(name));
            self.emit_op(OpCode::LoadGlobalRef);
            self.emit_u8(const_idx);
        }
    }

    /// Compile a function call argument. Bare identifiers use `emit_load_ref`
    /// (pass SharedRef by reference); all other expressions use the normal
    /// `compile_expr` (auto-deref SharedRef).
    fn compile_call_arg(&mut self, arg: &Expr) -> Result<(), InterpreterError> {
        if let Expr::Ident(name, _) = arg {
            self.emit_load_ref(name);
            Ok(())
        } else {
            self.compile_expr(arg)
        }
    }

    /// Resolve a variable name and emit a store (assignment) instruction.
    fn emit_store(&mut self, name: &str) {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(OpCode::AssignLocal);
            self.emit_u8(slot as u8);
        } else if let Some(idx) = self.resolve_upvalue(name) {
            self.emit_op(OpCode::AssignUpvalue);
            self.emit_u8(idx as u8);
        } else {
            let const_idx = self.emit_const(Value::str(name));
            self.emit_op(OpCode::AssignGlobal);
            self.emit_u8(const_idx);
        }
    }

    // --- function compilation ---

    /// Compile a function body into a sub-chunk. Returns (chunk_idx, upvalues).
    fn compile_function(
        &mut self,
        name: Option<&str>,
        params: &[Param],
        body: &Expr,
    ) -> Result<(usize, Vec<UpvalueInfo>), InterpreterError> {
        self.func_stack
            .push(FuncCtx::new(FunctionType::Function, name.map(|s| s.into())));

        // Declare params as locals (slots 0..arity).
        for param in params {
            self.declare_local(&param.name);
        }
        self.current_mut().chunk.arity = params.len();

        // Compile body.
        self.compile_expr(body)?;

        // Ensure Return is emitted.
        self.emit_op(OpCode::Return);

        // Pop FuncCtx.
        let func_ctx = self.func_stack.pop().expect("func_stack should not be empty");
        let upvalues = func_ctx.upvalues;
        let chunk = Rc::new(func_ctx.chunk);

        // Add to parent's sub_chunks.
        let parent = self.func_stack.last_mut().expect("no parent func ctx");
        let chunk_idx = parent.chunk.sub_chunks.len();
        parent.chunk.sub_chunks.push(chunk);

        Ok((chunk_idx, upvalues))
    }

    /// Emit Closure op + upvalue descriptors.
    fn emit_closure(&mut self, chunk_idx: usize, upvalues: &[UpvalueInfo]) {
        self.emit_op(OpCode::Closure);
        self.emit_u8(chunk_idx as u8);
        for uv in upvalues {
            self.emit_u8(if uv.is_local { 1 } else { 0 });
            self.emit_u8(uv.index as u8);
        }
    }

    /// Compile an actor's init chunk. The init chunk:
    /// - Evaluates `state x = v` decls as `DefineGlobal x` (the VM's
    ///   `DefineGlobal` stores into the actor env when running inside an
    ///   actor frame).
    /// - Compiles `fn` defs as closures (also `DefineGlobal`-ed into the
    ///   actor env).
    /// - Compiles each `on name(params) { body }` as a nested sub-chunk;
    ///   emits `Closure` + `RegisterHandler name_idx` so the runtime
    ///   collects the handler closure into the spawning actor's
    ///   `vm_handlers` map.
    ///
    /// The init chunk's arity matches the actor's constructor arity; the
    /// constructor args are bound as the first `arity` locals (params).
    /// Returns `(init_chunk_idx, upvalues)` so the parent can emit
    /// `DefineActor name, init_idx`.
    fn compile_actor_init(
        &mut self,
        name: &str,
        body: &[Stmt],
    ) -> Result<(usize, Vec<UpvalueInfo>), InterpreterError> {
        // 1y actor bodies don't declare explicit constructor params in the
        // current syntax (`actor Name { state ...; on ...; }`). The init
        // chunk has arity 0; if future syntax adds `actor Name(params)`,
        // this will change. State decls / fn defs / on-clauses are emitted
        // directly into the init chunk.
        self.func_stack
            .push(FuncCtx::new(FunctionType::Function, Some(name.to_string())));
        self.current_mut().chunk.arity = 0;

        // Emit each statement in the actor body.
        for stmt in body {
            match stmt {
                Stmt::StateDecl { name, value, .. } => {
                    // `state x = v` → evaluate v, DefineGlobal x (into actor env).
                    self.compile_expr(value)?;
                    let name_idx = self.emit_const(Value::str(name));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_u8(name_idx);
                }
                Stmt::FuncDef(fd) => {
                    // Compile the fn as a sub-chunk; emit Closure + DefineGlobal.
                    let (chunk_idx, upvalues) =
                        self.compile_function(Some(&fd.name), &fd.params, &fd.body)?;
                    self.emit_closure(chunk_idx, &upvalues);
                    let name_idx = self.emit_const(Value::str(&fd.name));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_u8(name_idx);
                }
                Stmt::OnClause(oc) => {
                    // Compile the handler body as a sub-chunk; emit Closure
                    // (which inherits the current frame's actor_env at
                    // runtime) + RegisterHandler name.
                    let (chunk_idx, upvalues) =
                        self.compile_function(Some(&oc.name), &oc.params, &oc.body)?;
                    self.emit_closure(chunk_idx, &upvalues);
                    let name_idx = self.emit_const(Value::str(&oc.name));
                    self.emit_op(OpCode::RegisterHandler);
                    self.emit_u8(name_idx);
                }
                _ => {
                    // Ignore other statements (Let, Expr, ...) in actor body
                    // for now — they're not part of the actor semantics.
                }
            }
        }

        // Init chunk returns Nil.
        self.emit_op(OpCode::Nil);
        self.emit_op(OpCode::Return);

        let func_ctx = self.func_stack.pop().expect("func_stack should not be empty");
        let upvalues = func_ctx.upvalues;
        let chunk = Rc::new(func_ctx.chunk);
        let parent = self.func_stack.last_mut().expect("no parent func ctx");
        let chunk_idx = parent.chunk.sub_chunks.len();
        parent.chunk.sub_chunks.push(chunk);
        Ok((chunk_idx, upvalues))
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), InterpreterError> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                self.compile_expr(value)?;
                let is_script = self.current().function_type == FunctionType::Script;
                let depth = self.current().scope_depth;
                if is_script && depth == 0 {
                    let const_idx = self.emit_const(Value::str(name));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_u8(const_idx);
                } else {
                    let slot = self.declare_local(name);
                    self.emit_op(OpCode::StoreLocal);
                    self.emit_u8(slot as u8);
                }
            }
            Stmt::SharedDecl { name, value, .. } => {
                self.compile_expr(value)?;
                self.emit_op(OpCode::SharedExpr);
                let is_script = self.current().function_type == FunctionType::Script;
                let depth = self.current().scope_depth;
                if is_script && depth == 0 {
                    let const_idx = self.emit_const(Value::str(name));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_u8(const_idx);
                } else {
                    let slot = self.declare_local(name);
                    self.emit_op(OpCode::StoreLocal);
                    self.emit_u8(slot as u8);
                }
            }
            Stmt::FuncDef(fd) => {
                let is_script = self.current().function_type == FunctionType::Script;
                let depth = self.current().scope_depth;

                // For recursion, declare the name as a local *before* compiling
                // the body, so body references resolve as upvalues.
                let name_slot = if is_script && depth == 0 {
                    None
                } else {
                    Some(self.declare_local(&fd.name))
                };

                let (chunk_idx, upvalues) =
                    self.compile_function(Some(&fd.name), &fd.params, &fd.body)?;
                self.emit_closure(chunk_idx, &upvalues);

                if let Some(slot) = name_slot {
                    self.emit_op(OpCode::StoreLocal);
                    self.emit_u8(slot as u8);
                } else {
                    let const_idx = self.emit_const(Value::str(&fd.name));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_u8(const_idx);
                }
            }
            Stmt::Expr(e) | Stmt::Semi(e) => {
                self.compile_expr(e)?;
                self.emit_op(OpCode::Pop);
            }
            Stmt::TypeDef(td) => {
                // Register the struct name so `Name({ ... })` calls compile
                // to `ConstructStruct`. Field types are parsed but not
                // enforced at runtime (mirrors the tree-walker).
                self.structs.insert(td.name.clone(), ());
            }
            Stmt::EnumDef(ed) => {
                // Register each variant in the compiler's variant table.
                // Zero-arity variants are also bound as globals
                // (Value::Variant) so they can be referenced by name.
                for variant in &ed.variants {
                    let arity = variant.fields.len();
                    self.variants.insert(variant.name.clone(), arity);
                    if arity == 0 {
                        let name_idx = self.emit_const(Value::str(&variant.name));
                        let v = Value::Variant {
                            name: Rc::new(variant.name.clone()),
                            args: Rc::new(vec![]),
                        };
                        let val_idx = self.emit_const(v);
                        self.emit_op(OpCode::Const);
                        self.emit_u8(val_idx);
                        self.emit_op(OpCode::DefineGlobal);
                        self.emit_u8(name_idx);
                    }
                }
            }
            Stmt::Import(imp) => {
                let path_idx = self.emit_const(Value::str(&imp.path));
                let alias_idx = match &imp.alias {
                    Some(a) => self.emit_const(Value::str(a)),
                    None => 0,
                };
                self.emit_op(OpCode::Import);
                self.emit_u8(path_idx as u8);
                self.emit_u8(alias_idx as u8);
                self.emit_u8(if imp.lazy { 1 } else { 0 });
            }
            Stmt::ActorDef(ad) => {
                // Compile the actor's init chunk: a sub-chunk holding
                // state decls, fn defs, and on-clauses (each on-clause's
                // body becomes a nested sub-chunk of the init chunk, and
                // is registered via RegisterHandler).
                let (init_idx, _) = self.compile_actor_init(&ad.name, &ad.body)?;
                let name_idx = self.emit_const(Value::str(&ad.name));
                self.emit_op(OpCode::DefineActor);
                self.emit_u8(name_idx);
                self.emit_u8(init_idx as u8);
            }
            _ => {
                // StateDecl and OnClause are only valid inside an actor
                // body; they are handled by compile_actor_init. Reaching
                // them here means a parse/AST invariant was violated.
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), InterpreterError> {
        self.compile_expr_inner(expr)
    }

    fn compile_expr_inner(&mut self, expr: &Expr) -> Result<(), InterpreterError> {
        match expr {
            Expr::Int(n, _) => {
                let idx = self.emit_const(Value::Int(n.clone()));
                self.emit_op(OpCode::Const);
                self.emit_u8(idx);
            }
            Expr::Decimal(d, _) => {
                let idx = self.emit_const(Value::Decimal(d.clone()));
                self.emit_op(OpCode::Const);
                self.emit_u8(idx);
            }
            Expr::Str(lit, _) => {
                // Simple string (one literal part).
                if lit.parts.len() == 1 {
                    if let StrPart::Literal(s) = &lit.parts[0] {
                        let idx = self.emit_const(Value::str(s));
                        self.emit_op(OpCode::Const);
                        self.emit_u8(idx);
                        return Ok(());
                    }
                }
                // Interpolated: build by concatenating parts with `+`.
                // For each Expr part, call `to_str(value)` to stringify.
                let mut first = true;
                for part in &lit.parts {
                    match part {
                        StrPart::Literal(s) => {
                            if s.is_empty() {
                                continue;
                            }
                            let idx = self.emit_const(Value::str(s));
                            self.emit_op(OpCode::Const);
                            self.emit_u8(idx);
                        }
                        StrPart::Expr(e) => {
                            // Push to_str, then the value, then call.
                            let f = self.emit_const(Value::str("to_str"));
                            self.emit_op(OpCode::LoadGlobal);
                            self.emit_u8(f);
                            self.compile_expr(e)?;
                            self.emit_op(OpCode::Call);
                            self.emit_u8(1);
                        }
                    }
                    if !first {
                        self.emit_op(OpCode::Add);
                    }
                    first = false;
                }
                if first {
                    // Empty string (all parts were empty literals).
                    let idx = self.emit_const(Value::str(""));
                    self.emit_op(OpCode::Const);
                    self.emit_u8(idx);
                }
            }
            Expr::Bool(b, _) => {
                self.emit_op(if *b { OpCode::True } else { OpCode::False });
            }
            Expr::Nil(_) => {
                self.emit_op(OpCode::Nil);
            }
            Expr::Ident(name, _) => {
                self.emit_load(name);
            }

            // --- collection literals ---
            Expr::VecLit { items, .. } => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.emit_op(OpCode::NewVec);
                self.emit_u8(items.len() as u8);
            }
            Expr::MapLit { entries, .. } => {
                for (k, v) in entries {
                    // Shorthand: a bare identifier key `{ x: 1 }` is treated
                    // as the string `"x"` (like JS/Rust struct-literal
                    // shorthand), not as a variable lookup. Mirrors the
                    // tree-walker.
                    match k {
                        Expr::Ident(name, _) => {
                            let idx = self.emit_const(Value::str(name));
                            self.emit_op(OpCode::Const);
                            self.emit_u8(idx);
                        }
                        _ => self.compile_expr(k)?,
                    }
                    // Preserve SharedRef when the value is a bare identifier
                    // bound to a shared cell — otherwise the cell would be
                    // auto-dereferenced here, breaking reference semantics
                    // (e.g. closures capturing a shared environment).
                    if let Expr::Ident(name, _) = v {
                        self.emit_load_ref(name);
                    } else {
                        self.compile_expr(v)?;
                    }
                }
                self.emit_op(OpCode::NewMap);
                self.emit_u8(entries.len() as u8);
            }
            Expr::SetLit { items, .. } => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.emit_op(OpCode::NewSet);
                self.emit_u8(items.len() as u8);
            }

            // --- binary ops ---
            Expr::BinOp { op, lhs, rhs, .. } => {
                self.compile_binop(*op, lhs, rhs)?;
            }
            Expr::UnaryOp { op, expr, .. } => {
                self.compile_expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit_op(OpCode::Neg),
                    UnaryOp::Not => self.emit_op(OpCode::Not),
                }
            }

            // --- pipe ---
            Expr::Pipe { lhs, rhs, .. } => {
                // a |> f(b) → f(a, b);  a |> f → f(a)
                match &**rhs {
                    Expr::Call { callee, args, .. } => {
                        self.compile_expr(callee)?;
                        self.compile_call_arg(lhs)?;
                        for arg in args {
                            self.compile_call_arg(arg)?;
                        }
                        self.emit_op(OpCode::Call);
                        self.emit_u8((args.len() + 1) as u8);
                    }
                    _ => {
                        self.compile_expr(rhs)?;
                        self.compile_call_arg(lhs)?;
                        self.emit_op(OpCode::Call);
                        self.emit_u8(1);
                    }
                }
            }

            // --- control flow ---
            Expr::If { cond, then, else_, .. } => {
                self.compile_expr(cond)?;
                let else_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.compile_expr(then)?;
                let end_jump = self.emit_jump(OpCode::Jump);
                self.patch_jump(else_jump);
                match else_ {
                    Some(e) => self.compile_expr(e)?,
                    None => self.emit_op(OpCode::Nil),
                }
                self.patch_jump(end_jump);
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.compile_match(scrutinee, arms)?;
            }

            // --- functions ---
            Expr::Lambda { params, body, .. } => {
                let (chunk_idx, upvalues) = self.compile_function(None, params, body)?;
                self.emit_closure(chunk_idx, &upvalues);
            }
            Expr::Block { stmts, tail, .. } => {
                self.begin_scope();
                for stmt in stmts {
                    self.compile_stmt(stmt)?;
                }
                match tail {
                    Some(e) => self.compile_expr(e)?,
                    None => self.emit_op(OpCode::Nil),
                }
                let n = self.end_scope();
                if n > 0 {
                    self.emit_op(OpCode::PopLocalKeep);
                    self.emit_u8(n as u8);
                }
            }
            Expr::Paren(e, _) => {
                self.compile_expr(e)?;
            }
            Expr::Call { callee, args, .. } => {
                // Check if this is an enum variant constructor.
                if let Expr::Ident(name, _) = &**callee {
                    if let Some(arity) = self.variants.get(name).copied() {
                        if arity > 0 {
                            for arg in args {
                                self.compile_expr(arg)?;
                            }
                            let name_idx = self.emit_const(Value::str(name));
                            self.emit_op(OpCode::ConstructVariant);
                            self.emit_u8(name_idx);
                            self.emit_u8(args.len() as u8);
                            return Ok(());
                        }
                    }
                    // Check if this is a struct constructor: `Name({ ... })`.
                    // Mirrors the tree-walker: exactly 1 argument (a Map),
                    // name registered via `type Name = { ... }`.
                    if self.structs.contains_key(name) {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        let name_idx = self.emit_const(Value::str(name));
                        self.emit_op(OpCode::ConstructStruct);
                        self.emit_u8(name_idx);
                        self.emit_u8(args.len() as u8);
                        return Ok(());
                    }
                }
                self.compile_expr(callee)?;
                for arg in args {
                    self.compile_call_arg(arg)?;
                }
                self.emit_op(OpCode::Call);
                self.emit_u8(args.len() as u8);
            }
            Expr::MethodCall { receiver, method, args, .. } => {
                // Runtime dispatch: if receiver is a Module, look up `method`
                // in its exports and call WITHOUT receiver; otherwise look up
                // `method` as a global and call WITH receiver as first arg.
                // Stack layout: [recv, arg0, ..., arg_{n-1}], then MethodCall.
                // The receiver and bare-identifier args use compile_call_arg
                // so a SharedRef is passed by reference (write-through).
                self.compile_call_arg(receiver)?;
                for arg in args {
                    self.compile_call_arg(arg)?;
                }
                let m_idx = self.emit_const(Value::str(method));
                self.emit_op(OpCode::MethodCall);
                self.emit_u8(m_idx);
                self.emit_u8(args.len() as u8);
            }

            // --- index / field ---
            Expr::Index { target, index, .. } => {
                self.compile_expr(target)?;
                self.compile_expr(index)?;
                self.emit_op(OpCode::Index);
            }
            Expr::Field { target, name, .. } => {
                self.compile_expr(target)?;
                let idx = self.emit_const(Value::str(name));
                self.emit_op(OpCode::Field);
                self.emit_u8(idx);
            }

            // --- assignment ---
            Expr::Assign { target, value, .. } => {
                match &**target {
                    Expr::Ident(name, _) => {
                        self.compile_expr(value)?;
                        self.emit_store(name);
                        self.emit_op(OpCode::Nil);
                    }
                    Expr::Index { target: obj, index, .. } => {
                        // `obj[idx] = value` ≡ `obj = assoc(obj, idx, value)`
                        // Compile order: value, obj, idx; then IndexAssign
                        // (pops idx, obj, value; pushes new_obj); then store
                        // new_obj back to the underlying binding; finally push
                        // nil (assignment evaluates to nil).
                        self.compile_expr(value)?;
                        self.compile_expr(obj)?;
                        self.compile_expr(index)?;
                        self.emit_op(OpCode::IndexAssign);
                        // Store the new collection back to `obj`'s binding.
                        // Only support the common case where obj is an Ident;
                        // nested targets (a.b[c] = v) would need recursive
                        // store-back which we don't implement yet.
                        match &**obj {
                            Expr::Ident(name, _) => {
                                self.emit_store(name);
                            }
                            _ => {
                                // Unsupported nested target: discard new_coll.
                                self.emit_op(OpCode::Pop);
                            }
                        }
                        self.emit_op(OpCode::Nil);
                    }
                    Expr::Field { target: obj, name, .. } => {
                        // `obj.field = value` — reuses IndexAssign with the
                        // field name as a string key. `ops::assoc` dispatches
                        // on the receiver type: Struct (full clone + insert)
                        // or Map (persistent update).
                        self.compile_expr(value)?;
                        self.compile_expr(obj)?;
                        let field_const = self.emit_const(Value::str(name));
                        self.emit_op(OpCode::Const);
                        self.emit_u8(field_const);
                        self.emit_op(OpCode::IndexAssign);
                        match &**obj {
                            Expr::Ident(name, _) => {
                                self.emit_store(name);
                            }
                            _ => {
                                self.emit_op(OpCode::Pop);
                            }
                        }
                        self.emit_op(OpCode::Nil);
                    }
                    _ => {
                        // Unsupported target: discard value, push nil.
                        self.compile_expr(value)?;
                        self.emit_op(OpCode::Pop);
                        self.emit_op(OpCode::Nil);
                    }
                }
            }
            Expr::CompoundAssign { op, target, value, .. } => {
                // target op= value → target = target op value
                match &**target {
                    Expr::Ident(name, _) => {
                        self.emit_load(name);
                        self.compile_expr(value)?;
                        self.emit_binop_simple(*op);
                        self.emit_store(name);
                        // Compound assignment evaluates to nil (like simple assign).
                        self.emit_op(OpCode::Nil);
                    }
                    _ => {
                        // Not supported for field/index targets in phase A.
                        self.emit_op(OpCode::Nil);
                    }
                }
            }

            // --- loops ---
            Expr::While { cond, body, .. } => {
                // Layout:
                //   PushLoop(continue=loop_start, break=break_handler)
                //   loop_start:
                //     cond; JumpIfFalse -> normal_handler
                //     body; Pop
                //     Loop -> loop_start
                //   normal_handler: PopLoop; Nil; Jump -> after
                //   break_handler:  PopLoop   (break value already on stack)
                //   after:
                //
                // PushLoop is emitted *outside* the loop body so it runs once.
                // Both normal exit and break exit go through a PopLoop so the
                // handler stack stays balanced. run_chunk does NOT pop the
                // handler on Break — it leaves that to break_handler's PopLoop.
                self.emit_op(OpCode::PushLoop);
                let continue_patch = self.current_code_len();
                self.emit_u16(0); // placeholder for continue_addr
                let break_patch = self.current_code_len();
                self.emit_u16(0); // placeholder for break_addr

                let loop_start = self.current_code_len();
                self.patch_u16(continue_patch, loop_start as u16);

                self.compile_expr(cond)?;
                let exit_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.compile_expr(body)?;
                self.emit_op(OpCode::Pop); // pop body value
                self.emit_loop(loop_start);

                // normal_handler: PopLoop, Nil, Jump -> after
                self.patch_jump(exit_jump);
                self.emit_op(OpCode::PopLoop);
                self.emit_op(OpCode::Nil); // while normal exit returns nil
                let jump_over_break = self.emit_jump(OpCode::Jump);

                // break_handler: PopLoop (break value already on stack)
                let break_addr = self.current_code_len() as u16;
                self.patch_u16(break_patch, break_addr);
                self.emit_op(OpCode::PopLoop);

                // after:
                self.patch_jump(jump_over_break);
            }
            Expr::Loop { body, .. } => {
                // Layout:
                //   PushLoop(continue=loop_start, break=break_handler)
                //   loop_start:
                //     body; Pop
                //     Loop -> loop_start
                //   break_handler: PopLoop  (break value is the loop result)
                //
                // Loop has no normal exit (infinite loop); only break exits,
                // and the break value becomes the loop's value.
                self.emit_op(OpCode::PushLoop);
                let continue_patch = self.current_code_len();
                self.emit_u16(0); // placeholder for continue_addr
                let break_patch = self.current_code_len();
                self.emit_u16(0); // placeholder for break_addr

                let loop_start = self.current_code_len();
                self.patch_u16(continue_patch, loop_start as u16);

                self.compile_expr(body)?;
                self.emit_op(OpCode::Pop); // pop body value
                self.emit_loop(loop_start);

                // break_handler: PopLoop (break value stays on stack as result)
                let break_addr = self.current_code_len() as u16;
                self.patch_u16(break_patch, break_addr);
                self.emit_op(OpCode::PopLoop);
                // No Nil — break value is the result. (This point is only
                // reachable via `break`; an infinite loop with no break never
                // gets here.)
            }
            Expr::Break { value, .. } => {
                // Push break value (nil if none), then emit Break signal.
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => self.emit_op(OpCode::Nil),
                }
                self.emit_op(OpCode::Break);
                self.emit_u16(1); // has_value = 1
                // Unreachable in well-formed code; emit Nil to keep stack balanced.
                self.emit_op(OpCode::Nil);
            }
            Expr::Continue { .. } => {
                self.emit_op(OpCode::Continue);
                self.emit_u16(0); // reserved
                // Unreachable; emit Nil to keep stack balanced.
                self.emit_op(OpCode::Nil);
            }
            Expr::For { var, iter, body, .. } => {
                // Desugar to index-based iteration with an explicit loop
                // handler. Layout:
                //
                //   begin_scope (for scope)
                //     StoreLocal __for_iter = iter_to_vec(iter)
                //     StoreLocal __for_i=0, __for_n=count(__for_iter)
                //     PushLoop(continue=inc_step, break=break_handler)
                //     loop_start:
                //       cond (i < n); JumpIfFalse -> normal_handler
                //       let var = get(__for_iter, i)
                //       begin_scope; body; Pop; end_scope; PopN inner_n
                //       PopN 1 (pop var); pop_local(var)
                //     inc_step:
                //       i = i + 1; Loop -> loop_start
                //     normal_handler: PopLoop; Nil; Jump -> after
                //     break_handler:  PopLoop; Pop (discard break value); Nil
                //     after:
                //   end_scope; PopN 3 (pop __for_iter, __for_i, __for_n)
                //
                // We materialize the iterable into a Vec via `iter_to_vec`
                // first, so that `count` + `get` indexing works uniformly
                // across Vec/Set/Map/Str (mirroring the tree-walker's
                // `iter_to_vec` semantics: Map → [k, v] pairs, Str → chars).
                //
                // For returns nil (tree-walker semantics: break value is
                // discarded). PushLoop is emitted outside the loop body so it
                // runs exactly once.
                self.begin_scope();

                // __for_iter = iter_to_vec(iter)
                let itv_idx = self.emit_const(Value::str("iter_to_vec"));
                self.emit_op(OpCode::LoadGlobal);
                self.emit_u8(itv_idx);
                self.compile_expr(iter)?;
                self.emit_op(OpCode::Call);
                self.emit_u8(1);
                let iter_slot = self.declare_local("__for_iter");
                self.emit_op(OpCode::StoreLocal);
                self.emit_u8(iter_slot as u8);

                // __for_i = 0
                let i_idx = self.emit_const(Value::int(0));
                self.emit_op(OpCode::Const);
                self.emit_u8(i_idx);
                let i_slot = self.declare_local("__for_i");
                self.emit_op(OpCode::StoreLocal);
                self.emit_u8(i_slot as u8);

                // __for_n = count(__for_iter)
                let count_idx = self.emit_const(Value::str("count"));
                self.emit_op(OpCode::LoadGlobal);
                self.emit_u8(count_idx);
                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(iter_slot as u8);
                self.emit_op(OpCode::Call);
                self.emit_u8(1);
                let n_slot = self.declare_local("__for_n");
                self.emit_op(OpCode::StoreLocal);
                self.emit_u8(n_slot as u8);

                // PushLoop outside the loop body (runs once).
                self.emit_op(OpCode::PushLoop);
                let continue_patch = self.current_code_len();
                self.emit_u16(0); // continue_addr placeholder
                let break_patch = self.current_code_len();
                self.emit_u16(0); // break_addr placeholder

                let loop_start = self.current_code_len();

                // cond: __for_i < __for_n
                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(i_slot as u8);
                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(n_slot as u8);
                self.emit_op(OpCode::Lt);
                let exit_jump = self.emit_jump(OpCode::JumpIfFalse);

                // body setup: let var = get(__for_iter, __for_i)
                let get_idx = self.emit_const(Value::str("get"));
                self.emit_op(OpCode::LoadGlobal);
                self.emit_u8(get_idx);
                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(iter_slot as u8);
                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(i_slot as u8);
                self.emit_op(OpCode::Call);
                self.emit_u8(2);
                let var_slot = self.declare_local(var);
                self.emit_op(OpCode::StoreLocal);
                self.emit_u8(var_slot as u8);

                // body
                self.begin_scope();
                self.compile_expr(body)?;
                self.emit_op(OpCode::Pop); // pop body value
                let inner_n = self.end_scope();
                if inner_n > 0 {
                    self.emit_op(OpCode::PopN);
                    self.emit_u8(inner_n as u8);
                }

                // Pop the var slot before the next iteration, and remove it
                // from the compiler's locals table so the for-scope end_scope
                // doesn't double-count it.
                self.emit_op(OpCode::PopN);
                self.emit_u8(1);
                self.pop_local();

                // inc_step: __for_i = __for_i + 1. continue_addr points here.
                let inc_target = self.current_code_len() as u16;
                self.patch_u16(continue_patch, inc_target);

                self.emit_op(OpCode::LoadLocal);
                self.emit_u8(i_slot as u8);
                let one_idx = self.emit_const(Value::int(1));
                self.emit_op(OpCode::Const);
                self.emit_u8(one_idx);
                self.emit_op(OpCode::Add);
                self.emit_op(OpCode::AssignLocal);
                self.emit_u8(i_slot as u8);

                self.emit_loop(loop_start);

                // normal_handler: PopLoop; Nil; Jump -> after
                self.patch_jump(exit_jump);
                self.emit_op(OpCode::PopLoop);
                self.emit_op(OpCode::Nil); // for normal exit returns nil
                let jump_over_break = self.emit_jump(OpCode::Jump);

                // break_handler: PopLoop; Pop (discard break value); Nil
                let break_addr = self.current_code_len() as u16;
                self.patch_u16(break_patch, break_addr);
                self.emit_op(OpCode::PopLoop);
                self.emit_op(OpCode::Pop); // discard break value (For returns nil)
                self.emit_op(OpCode::Nil);

                // after:
                self.patch_jump(jump_over_break);

                // Pop for-scope locals (__for_iter, __for_i, __for_n).
                // var was already removed via pop_local(), so outer_n is 3.
                let outer_n = self.end_scope();
                if outer_n > 0 {
                    self.emit_op(OpCode::PopN);
                    self.emit_u8(outer_n as u8);
                }
                // For's Nil result is already on the stack from the handlers.
            }

            // --- control transfer ---
            Expr::Return { value, .. } => {
                match value {
                    Some(e) => self.compile_expr(e)?,
                    None => self.emit_op(OpCode::Nil),
                }
                self.emit_op(OpCode::Return);
            }
            Expr::Raise { expr, .. } => {
                self.compile_expr(expr)?;
                self.emit_op(OpCode::Raise);
            }

            // --- shared ---
            Expr::SharedExpr { expr, .. } => {
                self.compile_expr(expr)?;
                self.emit_op(OpCode::SharedExpr);
            }

            // --- try / transact / retry ---
            Expr::Try { body, rescues, ensure, .. } => {
                // Layout (closure-based rescue — avoids local-slot conflicts
                // that arise when rescue bindings are declared as locals in
                // the middle of expression evaluation, where the parent
                // frame's stack already holds temporaries from the enclosing
                // expression):
                //
                //   PushTry(rescue_pc=RES, ensure_pc=ENS_or_0)
                //   <body>
                //   PopTry
                //   Jump -> ENS_or_AFTER
                // RES:                              // exc value on stack
                //   [for each typed rescue:]
                //     RescueMatch type_idx          // peek exc, push bool
                //     JumpIfFalse -> next           // pop bool; exc stays
                //     ClearPending
                //     [if bind:]  Closure; Swap; Call 1   // closure(exc)
                //     [else:]     Pop; Closure; Call 0   // closure()
                //     Jump -> ENS_or_AFTER
                //   next:
                //   [if catch-all:]
                //     ClearPending
                //     [if bind:]  Closure; Swap; Call 1
                //     [else:]     Pop; Closure; Call 0
                //     Jump -> ENS_or_AFTER
                //   [if no catch-all:]
                //     [if ensure:] Pop; Jump -> ENS
                //     [else:]      Raise
                // ENS_or_AFTER:
                //   [if ensure:] <ensure body> Pop EnsureExit
                //
                // The rescue body is compiled as a sub-chunk (closure) with
                // the bind name as its sole parameter (or 0 params if no
                // bind). This sidesteps the slot-allocation problem entirely:
                // the binding lives in the closure's own frame, not in the
                // parent frame where it would collide with temporaries.
                self.emit_op(OpCode::PushTry);
                let rescue_patch = self.current_code_len();
                self.emit_u16(0);
                let ensure_patch = self.current_code_len();
                self.emit_u16(0);

                self.compile_expr(body)?;

                // Normal exit path.
                self.emit_op(OpCode::PopTry);
                let jump_over_rescue = self.emit_jump(OpCode::Jump);

                // RES: rescue dispatcher.
                let rescue_addr = self.current_code_len() as u16;
                self.patch_u16(rescue_patch, rescue_addr);

                let mut rescue_body_exit_jumps: Vec<usize> = Vec::new();
                let mut next_clause_jump: Option<usize> = None;
                let has_catch_all = rescues.iter().any(|r| r.type_name.is_none());

                for rescue in rescues.iter() {
                    // Patch the previous JumpIfFalse (if any) to here.
                    if let Some(patch) = next_clause_jump.take() {
                        self.patch_jump(patch);
                    }
                    let is_catch_all = rescue.type_name.is_none();
                    if !is_catch_all {
                        let type_idx = match &rescue.type_name {
                            Some(tn) => self.emit_const(Value::str(tn)),
                            None => 0,
                        };
                        self.emit_op(OpCode::RescueMatch);
                        self.emit_u8(type_idx);
                        next_clause_jump = Some(self.emit_jump(OpCode::JumpIfFalse));
                        // JumpIfFalse already pops the bool; exc remains on top.
                    }
                    // This rescue clause has matched: clear the pending
                    // exception so a later `ensure` does not re-raise it.
                    self.emit_op(OpCode::ClearPending);

                    // Compile rescue body as a closure. The bind name (if
                    // any) becomes the closure's sole parameter, so the
                    // exception is passed as an argument rather than stored
                    // into a parent-frame local slot.
                    let params: Vec<Param> = match &rescue.bind {
                        Some(name) => vec![Param {
                            name: name.clone(),
                            type_annot: None,
                            span: crate::ast::Span::dummy(),
                        }],
                        None => vec![],
                    };
                    let (chunk_idx, upvalues) =
                        self.compile_function(Some("<rescue>"), &params, &rescue.body)?;

                    if rescue.bind.is_some() {
                        // exc on top. Push closure, swap, call with 1 arg.
                        self.emit_closure(chunk_idx, &upvalues);
                        self.emit_op(OpCode::Swap);
                        self.emit_op(OpCode::Call);
                        self.emit_u8(1);
                    } else {
                        // No bind: discard exc, call closure with 0 args.
                        self.emit_op(OpCode::Pop);
                        self.emit_closure(chunk_idx, &upvalues);
                        self.emit_op(OpCode::Call);
                        self.emit_u8(0);
                    }
                    rescue_body_exit_jumps.push(self.emit_jump(OpCode::Jump));
                }
                // Patch the last JumpIfFalse (if any) to here.
                if let Some(patch) = next_clause_jump.take() {
                    self.patch_jump(patch);
                }
                // After all clauses: if no catch-all, exc is still on stack.
                if !has_catch_all {
                    if ensure.is_some() {
                        // Discard exc, go to ensure (EnsureExit will re-raise
                        // from pending_exception).
                        self.emit_op(OpCode::Pop);
                        let j = self.emit_jump(OpCode::Jump);
                        rescue_body_exit_jumps.push(j);
                    } else {
                        // No ensure: re-raise immediately (exc on stack).
                        self.emit_op(OpCode::Raise);
                    }
                }

                // ENS_or_AFTER.
                let ens_addr = self.current_code_len() as u16;
                self.patch_jump(jump_over_rescue);
                for j in rescue_body_exit_jumps {
                    self.patch_jump(j);
                }
                if let Some(ensure_body) = ensure {
                    self.patch_u16(ensure_patch, ens_addr);
                    self.compile_expr(ensure_body)?;
                    self.emit_op(OpCode::Pop); // discard ensure result
                    self.emit_op(OpCode::EnsureExit);
                }
            }
            Expr::Transact { body, .. } => {
                // Layout:
                //   TRANSACT_START: PushTransact <self_addr=TRANSACT_START>
                //   <body>
                //   TransactCommit
                //   // fall through on success (result on stack)
                //
                // On conflict, TransactCommit increments the handler's
                // retry_count, pushes a fresh context, and jumps back to
                // TRANSACT_START. PushTransact sees the top handler's
                // retry_addr == self_addr and only pushes a fresh context
                // (no new handler). On `retry`, run_chunk catches the Retry
                // signal, pops the context, and jumps to retry_addr.
                let transact_start = self.current_code_len();
                self.emit_op(OpCode::PushTransact);
                self.emit_u16(transact_start as u16);
                self.compile_expr(body)?;
                self.emit_op(OpCode::TransactCommit);
            }
            Expr::Retry { .. } => {
                self.emit_op(OpCode::Retry);
            }

            // --- actor / async ---
            Expr::Spawn { name, args, .. } => {
                // Push args, then emit Spawn name nargs.
                for a in args {
                    self.compile_expr(a)?;
                }
                let name_idx = self.emit_const(Value::str(name));
                self.emit_op(OpCode::Spawn);
                self.emit_u8(name_idx);
                self.emit_u8(args.len() as u8);
            }
            Expr::ActorSend { actor, msg, .. } => {
                // `actor ! msg` — actor on stack, msg on top, then ActorSend.
                self.compile_expr(actor)?;
                self.compile_message(msg)?;
                self.emit_op(OpCode::ActorSend);
            }
            Expr::ActorRequest { actor, msg, .. } => {
                // `actor ? msg` — synchronous request/reply.
                self.compile_expr(actor)?;
                self.compile_message(msg)?;
                self.emit_op(OpCode::ActorRequest);
            }
            Expr::Reply { value, .. } => {
                // `reply expr` — evaluate value, emit Reply(1).
                self.compile_expr(value)?;
                self.emit_op(OpCode::Reply);
                self.emit_u16(1);
            }
            Expr::Yield { .. } => {
                // `yield` — drain all live actors' mailboxes.
                self.emit_op(OpCode::Yield);
            }
            Expr::Await { expr, .. } => {
                // Compile the task expression, then emit Await to poll it
                // synchronously and push the inner value.
                self.compile_expr(expr)?;
                self.emit_op(OpCode::Await);
            }
        }
        Ok(())
    }

    /// Compile a message expression for `actor ! msg` / `actor ? msg`.
    /// A bare identifier `Inc` becomes a zero-arg Variant. A call
    /// `Add(5, 3)` where the callee is an undefined identifier becomes a
    /// Variant with the call's args. Already-bound values (variants,
    /// ints, ...) are compiled as normal expressions.
    fn compile_message(&mut self, msg: &Expr) -> Result<(), InterpreterError> {
        match msg {
            Expr::Ident(name, _) => {
                // Always synthesize a zero-arg Variant for bare identifiers
                // in message position, matching the tree-walker's
                // `eval_message` (which checks if the name is bound first,
                // but we synthesize unconditionally — the runtime can
                // resolve via actor env if needed). To keep behavior
                // consistent, emit ConstructVariant with nargs=0.
                let name_idx = self.emit_const(Value::str(name));
                self.emit_op(OpCode::ConstructVariant);
                self.emit_u8(name_idx);
                self.emit_u8(0);
            }
            Expr::Call { callee, args, .. } => {
                // In message position, `Name(args)` is always a message
                // variant, never a function call. Match the tree-walker.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    let name_idx = self.emit_const(Value::str(name));
                    self.emit_op(OpCode::ConstructVariant);
                    self.emit_u8(name_idx);
                    self.emit_u8(args.len() as u8);
                } else {
                    // Non-Ident callee: compile as a normal expression.
                    self.compile_expr(msg)?;
                }
            }
            _ => {
                // Any other expression: compile normally.
                self.compile_expr(msg)?;
            }
        }
        Ok(())
    }

    fn compile_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(), InterpreterError> {
        // Short-circuit `and` / `or`.
        match op {
            BinOp::And => {
                self.compile_expr(lhs)?;
                self.emit_op(OpCode::Dup);
                let jump = self.emit_jump(OpCode::JumpIfFalse);
                // lhs truthy: pop the dup, push rhs.
                self.emit_op(OpCode::Pop);
                self.compile_expr(rhs)?;
                self.patch_jump(jump);
                return Ok(());
            }
            BinOp::Or => {
                self.compile_expr(lhs)?;
                self.emit_op(OpCode::Dup);
                let jump = self.emit_jump(OpCode::JumpIfTrue);
                // lhs falsy: pop the dup, push rhs.
                self.emit_op(OpCode::Pop);
                self.compile_expr(rhs)?;
                self.patch_jump(jump);
                return Ok(());
            }
            _ => {}
        }

        self.compile_expr(lhs)?;
        self.compile_expr(rhs)?;
        self.emit_binop_simple(op);
        Ok(())
    }

    fn emit_binop_simple(&mut self, op: BinOp) {
        let code = match op {
            BinOp::Add => OpCode::Add,
            BinOp::Sub => OpCode::Sub,
            BinOp::Mul => OpCode::Mul,
            BinOp::Div => OpCode::Div,
            BinOp::Mod => OpCode::Mod,
            BinOp::Eq => OpCode::Eq,
            BinOp::Neq => OpCode::Neq,
            BinOp::Lt => OpCode::Lt,
            BinOp::Gt => OpCode::Gt,
            BinOp::Lte => OpCode::Lte,
            BinOp::Gte => OpCode::Gte,
            BinOp::And | BinOp::Or => unreachable!("handled by short-circuit"),
        };
        self.emit_op(code);
    }

    // -----------------------------------------------------------------------
    // Match compilation
    // -----------------------------------------------------------------------

    fn compile_match(&mut self, scrutinee: &Expr, arms: &[MatchArm]) -> Result<(), InterpreterError> {
        // Compile scrutinee and store in a local.
        self.compile_expr(scrutinee)?;
        let scrutinee_slot = self.declare_local("__match_scrutinee");
        self.emit_op(OpCode::StoreLocal);
        self.emit_u8(scrutinee_slot as u8);

        let mut end_jumps: Vec<usize> = Vec::new();

        for arm in arms {
            // LoadLocal scrutinee
            self.emit_op(OpCode::LoadLocal);
            self.emit_u8(scrutinee_slot as u8);

            // Match(pattern)
            let pat_idx = self.add_pattern(&arm.pattern);
            let n_bindings = pattern_binding_count(&arm.pattern);
            self.emit_op(OpCode::Match);
            self.emit_u8(pat_idx);

            // JumpIfFalse arm_end
            let match_fail_jump = self.emit_jump(OpCode::JumpIfFalse);

            // Declare bindings as locals.
            let binding_names = pattern_binding_names(&arm.pattern);
            for name in &binding_names {
                self.declare_local(name);
            }

            // Guard
            if let Some(guard) = &arm.guard {
                self.compile_expr(guard)?;
                let guard_fail_jump = self.emit_jump(OpCode::JumpIfFalse);

                // Body (guard passed)
                self.compile_expr(&arm.body)?;
                self.emit_op(OpCode::PopLocalKeep);
                self.emit_u8(n_bindings as u8);
                let end_jump = self.emit_jump(OpCode::Jump);
                end_jumps.push(end_jump);

                // Guard failed: pop bindings
                self.patch_jump(guard_fail_jump);
                self.emit_op(OpCode::PopN);
                self.emit_u8(n_bindings as u8);
                // Fall through to arm_end
            } else {
                // Body (no guard)
                self.compile_expr(&arm.body)?;
                self.emit_op(OpCode::PopLocalKeep);
                self.emit_u8(n_bindings as u8);
                let end_jump = self.emit_jump(OpCode::Jump);
                end_jumps.push(end_jump);
            }

            // Pop binding locals from compiler tracking.
            for _ in &binding_names {
                self.pop_local();
            }

            // arm_end (match fail jump target — next arm starts here)
            self.patch_jump(match_fail_jump);
        }

        // No arm matched: push nil.
        self.emit_op(OpCode::Nil);

        // match_end: patch all end jumps to here.
        for jump in &end_jumps {
            self.patch_jump(*jump);
        }

        // Pop scrutinee local, keep match result.
        self.emit_op(OpCode::PopLocalKeep);
        self.emit_u8(1);
        self.pop_local();

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pattern helpers
// ---------------------------------------------------------------------------

fn pattern_binding_count(pattern: &Pattern) -> usize {
    pattern_binding_names(pattern).len()
}

fn pattern_binding_names(pattern: &Pattern) -> Vec<String> {
    let mut names = Vec::new();
    collect_binding_names(pattern, &mut names);
    names
}

fn collect_binding_names(pattern: &Pattern, names: &mut Vec<String>) {
    match pattern {
        Pattern::Wildcard(_) | Pattern::Lit(_, _) => {}
        Pattern::Bind(name, _) => names.push(name.clone()),
        Pattern::Variant { args, .. } => {
            for arg in args {
                collect_binding_names(arg, names);
            }
        }
        Pattern::Struct { fields, .. } => {
            for (_, fpat) in fields {
                collect_binding_names(fpat, names);
            }
        }
        Pattern::Vec { pats, .. } => {
            for pat in pats {
                collect_binding_names(pat, names);
            }
        }
        Pattern::Or(pats, _) => {
            if let Some(first) = pats.first() {
                collect_binding_names(first, names);
            }
        }
        Pattern::Guard(pat, _, _) => {
            collect_binding_names(pat, names);
        }
    }
}
