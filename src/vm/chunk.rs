//! Bytecode instruction set and chunk structure for the 1y VM.
//!
//! A [`Chunk`] is a compiled function body: a flat byte stream of opcodes +
//! operands, plus a constant pool, a pattern pool (for `match`), an exception
//! table (for `try/rescue/ensure`), and sub-chunks (one per nested function
//! body / lambda / handler).
//!
//! # Instruction encoding
//!
//! Every instruction is 1 opcode byte followed by fixed-width operands:
//! - `u8`  — 1 byte (slot index, const index, nargs, ...)
//! - `u16` — 2 bytes little-endian (jump target = absolute offset into `code`)
//!
//! The compiler emits, the VM decodes. Width is determined by the opcode.

use crate::ast::Pattern;
use crate::value::Value;
use std::rc::Rc;

/// One entry in a chunk's exception table.
///
/// When an exception (user `raise`, or a type/runtime error) is raised while
/// `ip` is in `[try_start, try_end)`, control transfers to `handler_pc` with
/// the exception value on top of the stack. `ensure_pc`, if set, is run on
/// exit from the protected region (whether normal or exceptional) — it is the
/// address of the `ensure` body, which the compiler arranges to run via a
/// separate mechanism (recorded here so unwinding can find it).
#[derive(Clone, Debug)]
pub struct ExceptionHandler {
    /// Start of the protected `try` body (inclusive).
    pub try_start: u16,
    /// End of the protected `try` body (exclusive).
    pub try_end: u16,
    /// Address of the `rescue` dispatcher. The exception value is already on
    /// the stack; the dispatcher tests each rescue clause's type and jumps.
    /// `None` means no rescue (only `ensure`).
    pub rescue_pc: Option<u16>,
    /// Address of the `ensure` body, run on any exit from the region.
    pub ensure_pc: Option<u16>,
    /// Address to resume at after the try region completes normally.
    /// (Used to skip ensure when falling through normally — the compiler
    /// instead emits an explicit jump; this field is reserved/unused for now
    /// and kept for future structured unwinding.)
    pub _continue_pc: Option<u16>,
}

/// A compiled function body.
#[derive(Debug)]
pub struct Chunk {
    /// The bytecode — opcodes + inline operands.
    pub code: Vec<u8>,
    /// Constant pool: literals (Int/Decimal/Str/...) and global-name strings.
    pub consts: Vec<Value>,
    /// `code.len()` entries; `spans[i]` is the source span of the instruction
    /// starting at byte `i`. Used for error reporting.
    pub spans: Vec<crate::ast::Span>,
    /// Pattern pool: `match` patterns referenced by `OpCode::Match`.
    pub patterns: Vec<Rc<Pattern>>,
    /// Exception handlers covering `try` regions.
    pub exception_table: Vec<ExceptionHandler>,
    /// Sub-chunks: one per nested function body (FuncDef/Lambda/OnClause).
    /// `OpCode::Closure` references these by index.
    pub sub_chunks: Vec<Rc<Chunk>>,
    /// Number of parameters. The first `arity` local slots hold arguments.
    pub arity: usize,
    /// Number of upvalues this chunk's closures expect. The
    /// `OpCode::Closure` operand is followed by `upvalue_count` pairs of
    /// `(is_local: u8, index: u8)` describing each upvalue.
    pub upvalue_count: usize,
    /// Function name, for error messages.
    pub name: Option<String>,
}

impl Chunk {
    pub fn new(name: Option<String>, arity: usize) -> Self {
        Chunk {
            code: Vec::new(),
            consts: Vec::new(),
            spans: Vec::new(),
            patterns: Vec::new(),
            exception_table: Vec::new(),
            sub_chunks: Vec::new(),
            arity,
            upvalue_count: 0,
            name,
        }
    }
}

/// Bytecode opcodes.
///
/// Operand widths are documented per variant. The VM's dispatch reads the
/// opcode byte then the documented operands.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    // --- literals ---
    /// push nil
    Nil = 0,
    /// push true
    True,
    /// push false
    False,
    /// u8 const_idx — push consts[idx]
    Const,

    // --- locals / upvalues / globals ---
    /// u8 slot — push local slot
    LoadLocal,
    /// u8 slot — pop and store into local slot
    StoreLocal,
    /// u8 idx — push upvalue[idx]
    LoadUpvalue,
    /// u8 idx — pop and store into upvalue[idx]
    StoreUpvalue,
    /// u8 name_const_idx — push global named consts[name_idx] (Str)
    LoadGlobal,
    /// u8 name_const_idx — pop and assign existing global
    StoreGlobal,
    /// u8 name_const_idx — pop and define new global
    DefineGlobal,
    /// u8 slot — push local slot WITHOUT auto-derefing SharedRef.
    /// Used for bare-identifier function call args so that a SharedRef
    /// parameter is passed by reference (mirrors the tree-walker's
    /// call-site fix for SharedRef write-through).
    LoadLocalRef,
    /// u8 idx — push upvalue[idx] WITHOUT auto-derefing SharedRef.
    LoadUpvalueRef,
    /// u8 name_const_idx — push global WITHOUT auto-derefing SharedRef.
    LoadGlobalRef,

    // --- stack ---
    /// pop top
    Pop,
    /// u8 n — pop n values
    PopN,
    /// u8 n — pop n locals *below* the top value, keeping the top value.
    /// Used to end a block scope: the block's tail value sits on top, with
    /// the block's locals underneath; this removes the locals and slides the
    /// tail value down.
    PopLocalKeep,
    /// duplicate top
    Dup,

    // --- collection literals ---
    /// u8 n — pop n, push Vec (first pushed = first element)
    NewVec,
    /// u8 n — pop 2n (n key-value pairs, first pushed = first key), push Map
    NewMap,
    /// u8 n — pop n, push Set
    NewSet,

    // --- arithmetic / comparison ---
    Add, Sub, Mul, Div, Mod,
    Neg, Not,
    Eq, Neq, Lt, Gt, Lte, Gte,

    // --- control flow ---
    /// u16 addr — unconditional jump
    Jump,
    /// u16 addr — pop, jump if falsy
    JumpIfFalse,
    /// u16 addr — pop, jump if truthy
    JumpIfTrue,
    /// u16 addr — backward jump (loop)
    Loop,

    // --- functions ---
    /// u8 chunk_idx — push closure; followed by upvalue_count * (u8 is_local,
    /// u8 index) describing each upvalue.
    Closure,
    /// u8 nargs — call top-of-stack callable with nargs args (args below callee)
    Call,
    /// u8 method_const_idx, u8 nargs — runtime dispatch: stack is
    /// [recv, arg0, ..., arg_{nargs-1}]. If recv is a Module, look up
    /// `method` in its exports and call WITHOUT recv; otherwise look up
    /// `method` as a global and call WITH recv as the first argument.
    MethodCall,
    /// u8 name_const_idx, u8 nargs — pop nargs args, push
    /// Value::Variant { name, args }. Used to construct enum variants
    /// with arity > 0. Zero-arity variants are bound as globals via
    /// DefineGlobal.
    ConstructVariant,
    /// u8 name_const_idx, u8 nargs — pop nargs args, build a
    /// Value::Struct with the given name. nargs must be 1 and the
    /// argument must be a Map (string-keyed); its entries become the
    /// struct's fields. Mirrors the tree-walker's struct construction.
    ConstructStruct,
    /// pop top as return value; pop frame
    Return,

    // --- assignment (by target kind) ---
    /// u8 slot — pop, assign to local
    AssignLocal,
    /// u8 idx — pop, assign to upvalue
    AssignUpvalue,
    /// u8 name_const_idx — pop, assign to global
    AssignGlobal,

    // --- index / field access ---
    /// pop idx, pop target, push target[idx]
    Index,
    /// pop index, pop collection, pop value; compute new collection =
    /// `assoc(coll, idx, value)`; push new collection. Caller is responsible
    /// for storing the new collection back into the underlying variable.
    IndexAssign,
    /// u8 name_const_idx — pop target, push target.field
    Field,
    /// u8 name_const_idx — pop value, pop target, set target.field = value, push nil
    FieldAssign,

    // --- pattern matching ---
    /// u8 pattern_idx — pop value; if match, push N bindings (compiler-known)
    /// then push true; else push false.
    Match,

    // --- control-transfer signals (encoded as exceptions in phase A) ---
    /// break out of current loop. u16 has_value (0/1). If 1, value is on top.
    /// Implemented by raising a Break signal caught by the loop's handler.
    Break,
    /// continue current loop. u16 0 (unused, reserved).
    Continue,
    /// u16 has_value. return from current function.
    ReturnSignal,
    /// retry current transaction.
    Retry,

    // --- exceptions ---
    /// pop value, raise as user exception
    Raise,
    /// u16 rescue_pc, u16 ensure_pc — push an exception handler onto the
    /// VM's handler stack. `rescue_pc` is the address of the rescue
    /// dispatcher (or 0 if none); `ensure_pc` is the address of the ensure
    /// body (or 0 if none). The handler is popped by `PopTry` on normal exit.
    PushTry,
    /// Pop the top exception handler (on normal exit from the try region).
    PopTry,
    /// u8 type_name_const_idx (0 = catch-all) — peek the exception value
    /// on top of the stack, test if it matches `type_name` (by
    /// Variant/Struct name; 0 = match anything), push bool result.
    /// The exception value remains on the stack.
    RescueMatch,
    /// Called at the end of an ensure body. If there is a pending
    /// exception (set by `raise`), re-raise it; otherwise fall through
    /// (normal exit from the try region).
    EnsureExit,
    /// Clear the pending exception flag. Emitted at the start of a rescue
    /// body that has successfully matched (so a later `ensure` knows the
    /// exception was handled and should not be re-raised).
    ClearPending,

    // --- shared / transact (basic in phase A; full STM in phase B) ---
    /// pop value, push SharedRef wrapping it (shared expr)
    SharedExpr,
    /// u16 self_addr — push a transaction handler (retry_addr=self_addr,
    /// stack_depth=current, retry_count=0) and a fresh TransactionContext.
    /// If the top handler already has retry_addr == self_addr (i.e. this is
    /// a retry), only push a fresh context (handler stays).
    PushTransact,
    /// pop a TransactionContext; commit it. On success (no conflict or
    /// nested merge): pop the transact handler, fall through with the body
    /// result on the stack. On conflict (outermost only): increment the
    /// handler's retry_count (error if > MAX), push a fresh context, and
    /// jump to the handler's retry_addr.
    TransactCommit,

    // --- actor / async (phase C) ---
    Yield,
    /// u16 has_value. reply in actor handler.
    Reply,
    Await,
    /// u8 actor_name_const_idx, u8 nargs — instantiate actor `actor_name`
    /// with `nargs` constructor args (args below opcode). Pushes Value::Actor.
    Spawn,
    /// pop msg, pop actor; enqueue `msg` in actor's mailbox (fire-and-forget).
    /// Pushes Nil.
    ActorSend,
    /// pop msg, pop actor; synchronously dispatch `msg` to the actor's
    /// matching handler and wait for `reply`. Pushes the reply value.
    ActorRequest,
    /// u8 name_const_idx, u8 chunk_idx — register the actor definition's
    /// init chunk under `name` in the VM's `actor_defs` table. Emitted at
    /// the top level when an `actor Name { ... }` definition is compiled.
    DefineActor,
    /// u8 name_const_idx — pop a Closure value, register it as a handler
    /// in the currently-spawning actor's `vm_handlers` map. Only valid
    /// inside an actor init chunk.
    RegisterHandler,

    // --- module system ---
    /// u8 path_const_idx, u8 alias_const_idx (0 = no alias), u8 lazy (0/1).
    /// Loads a module and binds it under the alias (or last path segment).
    Import,

    // --- stack manipulation ---
    /// swap top two values
    Swap,
    /// rotate top three: [a, b, c] → [b, c, a] (used for index assign)
    Rot3,

    // --- loop handlers (for break/continue unwinding) ---
    /// u16 continue_addr, u16 break_addr — push a loop handler onto the
    /// VM's loop stack. Break/Continue signals unwind to the nearest handler.
    PushLoop,
    /// pop the top loop handler (on normal loop exit).
    PopLoop,

    /// sentinel for end-of-table — must stay last.
    _End,
}

impl OpCode {
    /// Number of operand bytes following this opcode, *not counting* the
    /// variable-length upvalue descriptors after `Closure`.
    pub fn operand_bytes(self) -> usize {
        match self {
            OpCode::Nil | OpCode::True | OpCode::False
            | OpCode::Pop | OpCode::Dup
            | OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod
            | OpCode::Neg | OpCode::Not
            | OpCode::Eq | OpCode::Neq | OpCode::Lt | OpCode::Gt | OpCode::Lte | OpCode::Gte
            | OpCode::Return | OpCode::Index | OpCode::IndexAssign
            | OpCode::Raise | OpCode::SharedExpr
            | OpCode::Yield | OpCode::Await | OpCode::Retry
            | OpCode::Swap | OpCode::Rot3
            | OpCode::PopLoop | OpCode::PopTry | OpCode::EnsureExit | OpCode::ClearPending
            | OpCode::TransactCommit
            | OpCode::ActorSend | OpCode::ActorRequest
            | OpCode::_End => 0,
            OpCode::Const
            | OpCode::LoadLocal | OpCode::StoreLocal
            | OpCode::LoadUpvalue | OpCode::StoreUpvalue
            | OpCode::LoadGlobal | OpCode::StoreGlobal | OpCode::DefineGlobal
            | OpCode::LoadLocalRef | OpCode::LoadUpvalueRef | OpCode::LoadGlobalRef
            | OpCode::PopN | OpCode::PopLocalKeep
            | OpCode::NewVec | OpCode::NewMap | OpCode::NewSet
            | OpCode::Call
            | OpCode::AssignLocal | OpCode::AssignUpvalue | OpCode::AssignGlobal
            | OpCode::Field | OpCode::FieldAssign
            | OpCode::Match
            | OpCode::RescueMatch
            | OpCode::Closure
            | OpCode::RegisterHandler => 1,
            OpCode::MethodCall => 2,
            OpCode::ConstructVariant => 2,
            OpCode::ConstructStruct => 2,
            OpCode::PushTry => 4,
            OpCode::Import => 3,
            OpCode::PushLoop => 4,
            OpCode::PushTransact => 2,
            OpCode::Spawn => 2,
            OpCode::DefineActor => 2,
            OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Loop
            | OpCode::Break | OpCode::Continue | OpCode::ReturnSignal | OpCode::Reply => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Encode/decode helpers shared by compiler and VM.
// ---------------------------------------------------------------------------

#[inline]
pub fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

#[inline]
pub fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.push((v & 0xff) as u8);
    buf.push((v >> 8) as u8);
}

#[inline]
pub fn read_u8(code: &[u8], ip: &mut usize) -> u8 {
    let v = code[*ip];
    *ip += 1;
    v
}

#[inline]
pub fn read_u16(code: &[u8], ip: &mut usize) -> u16 {
    let lo = code[*ip] as u16;
    let hi = code[*ip + 1] as u16;
    *ip += 2;
    lo | (hi << 8)
}
