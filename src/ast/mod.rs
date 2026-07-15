//! Abstract syntax tree definitions for the `1y` language.
//!
//! Everything is designed around two principles:
//!   1. Every node carries a [`Span`] for precise error reporting.
//!   2. "Everything is an expression" — declarations are [`Stmt`] items that
//!      can appear inside a block, while control flow (`if`/`match`/blocks/
//!      `try`/`transact`) and control transfers (`raise`/`return`/`reply`)
//!      are [`Expr`] variants.

pub mod span;

pub use span::{Pos, Span, Spanned};

use bigdecimal::BigDecimal;
use num_bigint::BigInt;

/// A complete source file: a sequence of top-level statements.
#[derive(Debug, Clone)]
pub struct Program {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Statements / declarations
// ---------------------------------------------------------------------------

/// A statement. Either a declaration (binding, function, type, actor, ...) or
/// an expression optionally terminated with `;`.
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let name: T = value`
    Let {
        name: String,
        type_annot: Option<TypeAnnot>,
        value: Expr,
        span: Span,
    },
    /// `shared name: T = value` — versioned mutable variable (MVCC/STM).
    SharedDecl {
        name: String,
        type_annot: Option<TypeAnnot>,
        value: Expr,
        span: Span,
    },
    /// `state name = value` — only valid inside an `actor` body.
    StateDecl {
        name: String,
        type_annot: Option<TypeAnnot>,
        value: Expr,
        span: Span,
    },
    /// `on name(params) -> T { ... }` — actor message handler.
    OnClause(OnClause),
    FuncDef(FuncDef),
    /// `type Name<...> = { field: T, ... }` — record/struct type.
    TypeDef(TypeDef),
    /// `enum Name<...> { Variant(fields), ... }` — algebraic data type.
    EnumDef(EnumDef),
    ActorDef(ActorDef),
    Import(Import),
    /// An expression with no trailing semicolon. When it is the last item of
    /// a block it becomes the block's value.
    Expr(Expr),
    /// An expression followed by `;` (value discarded).
    Semi(Expr),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::SharedDecl { span, .. }
            | Stmt::StateDecl { span, .. } => *span,
            Stmt::OnClause(o) => o.span,
            Stmt::FuncDef(f) => f.span,
            Stmt::TypeDef(t) => t.span,
            Stmt::EnumDef(e) => e.span,
            Stmt::ActorDef(a) => a.span,
            Stmt::Import(i) => i.span,
            Stmt::Expr(e) | Stmt::Semi(e) => e.span(),
        }
    }
}

/// `fn name(params) -> T { body }`
#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeAnnot>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// A function/actor-handler parameter.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_annot: Option<TypeAnnot>,
    pub span: Span,
}

/// `type Name<T, U> = { field: T, ... }`
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<(String, TypeAnnot)>,
    pub span: Span,
}

/// A single variant of an [`EnumDef`].
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    /// Field types, e.g. `Circle(Number)` -> one field of type `Number`.
    pub fields: Vec<TypeAnnot>,
    pub span: Span,
}

/// `enum Name<T> { Some(T), None }`
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<VariantDef>,
    pub span: Span,
}

/// `on name(params) -> T { body }` inside an actor.
#[derive(Debug, Clone)]
pub struct OnClause {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeAnnot>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// `actor Name { state ...; on ...; }`
#[derive(Debug, Clone)]
pub struct ActorDef {
    pub name: String,
    /// Body restricted to [`Stmt::StateDecl`] / [`Stmt::OnClause`] / [`Stmt::FuncDef`].
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// `import path` / `import path as alias` / `lazy import path`
#[derive(Debug, Clone)]
pub struct Import {
    /// Dotted module path, e.g. `"net.socket"`.
    pub path: String,
    pub alias: Option<String>,
    pub lazy: bool,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Expr {
    /// Arbitrary-precision integer literal.
    Int(BigInt, Span),
    /// Arbitrary-precision decimal literal.
    Decimal(BigDecimal, Span),
    /// String literal, possibly with interpolated expressions.
    Str(StrLit, Span),
    Bool(bool, Span),
    Nil(Span),
    Ident(String, Span),

    /// `[a, b, c]` — persistent vector literal.
    VecLit { items: Vec<Expr>, span: Span },
    /// `{ k1: v1, k2: v2 }` — persistent map literal.
    MapLit { entries: Vec<(Expr, Expr)>, span: Span },
    /// `#{1, 2, 3}` — persistent set literal.
    SetLit { items: Vec<Expr>, span: Span },

    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    /// `lhs |> rhs` — pipe (passes lhs as first arg of the rhs call).
    Pipe {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },

    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Option<Box<Expr>>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },

    /// `fn(params) -> T { body }` (anonymous; named functions are [`Stmt::FuncDef`]).
    Lambda {
        params: Vec<Param>,
        return_type: Option<TypeAnnot>,
        body: Box<Expr>,
        span: Span,
    },
    /// `{ stmt; stmt; tail }`. `tail` is `None` if the block ends with `;` or
    /// is empty (value is then `nil`).
    Block {
        stmts: Vec<Stmt>,
        tail: Option<Box<Expr>>,
        span: Span,
    },
    Paren(Box<Expr>, Span),

    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// `receiver.method(args)` — desugared to a call on the receiver's method.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        span: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// `target.field`
    Field {
        target: Box<Expr>,
        name: String,
        span: Span,
    },

    /// `actor ! msg` — fire-and-forget send.
    ActorSend {
        actor: Box<Expr>,
        msg: Box<Expr>,
        span: Span,
    },
    /// `actor ? msg` — request/reply (blocks until `reply(...)`).
    ActorRequest {
        actor: Box<Expr>,
        msg: Box<Expr>,
        span: Span,
    },
    /// `spawn Name(args)`
    Spawn {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },

    /// `raise expr`
    Raise { expr: Box<Expr>, span: Span },
    /// `try body rescue T as e { ... } [ensure { ... }]`
    Try {
        body: Box<Expr>,
        rescues: Vec<RescueClause>,
        ensure: Option<Box<Expr>>,
        span: Span,
    },
    /// `transact { ... }` — explicit optimistic transaction block.
    Transact { body: Box<Expr>, span: Span },
    /// `retry` — abort the current transaction attempt and restart.
    /// Only valid inside a `transact` block.
    Retry { span: Span },

    /// `return [expr]`
    Return { value: Option<Box<Expr>>, span: Span },
    /// `reply expr` — only valid inside an `on` handler.
    Reply { value: Box<Expr>, span: Span },
    /// `yield` — drain all pending `!` messages from live actors' mailboxes.
    /// Returns `nil`. Used in event loops (e.g. HTTP server) to process
    /// fire-and-forget messages without waiting for program exit.
    Yield { span: Span },
    /// `await expr` — suspend the current coroutine until the Task completes.
    /// If the Task is already complete, returns immediately (sync path).
    /// Any function can use `await` — there is no function coloring (Zig-style).
    Await { expr: Box<Expr>, span: Span },
    /// `shared expr` — create a shared transactional cell wrapping the value.
    /// Returns a `Value::Shared`. Can be used as an expression:
    /// `let x = shared 0;` or as a statement: `shared x = 0;`.
    SharedExpr { expr: Box<Expr>, span: Span },

    /// `target = value` — assignment (returns `nil`). `target` is an lvalue
    /// ([`Expr::Ident`], [`Expr::Field`], [`Expr::Index`]).
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    /// `target += value` (and `-=`, `*=`, `/=`, `%=`).
    /// Semantically equivalent to `target = target <op> value`.
    CompoundAssign {
        op: BinOp,
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    /// `while cond { body }` — loops while `cond` is truthy. Returns `nil`.
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// `for x in iter { body }` — iterates over a Vec, Map, Set, or Str.
    /// `x` is bound to each element (for Map: a `[key, value]` pair).
    /// Returns `nil`.
    For {
        var: String,
        iter: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// `loop { body }` — infinite loop; exits via `break expr`.
    /// Returns the value passed to `break`, or `nil` if `break` has no value.
    Loop {
        body: Box<Expr>,
        span: Span,
    },
    /// `break [expr]` — exits the enclosing `while`/`for`/`loop`.
    /// The optional value becomes the loop's result.
    Break {
        value: Option<Box<Expr>>,
        span: Span,
    },
    /// `continue` — skips to the next iteration of the enclosing loop.
    Continue { span: Span },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Decimal(_, s)
            | Expr::Str(_, s)
            | Expr::Bool(_, s)
            | Expr::Nil(s)
            | Expr::Ident(_, s)
            | Expr::Pipe { span: s, .. }
            | Expr::BinOp { span: s, .. }
            | Expr::UnaryOp { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::Match { span: s, .. }
            | Expr::Lambda { span: s, .. }
            | Expr::Block { span: s, .. }
            | Expr::Paren(_, s)
            | Expr::Call { span: s, .. }
            | Expr::MethodCall { span: s, .. }
            | Expr::Index { span: s, .. }
            | Expr::Field { span: s, .. }
            | Expr::ActorSend { span: s, .. }
            | Expr::ActorRequest { span: s, .. }
            | Expr::Spawn { span: s, .. }
            | Expr::Raise { span: s, .. }
            | Expr::Try { span: s, .. }
            | Expr::Transact { span: s, .. }
            | Expr::Retry { span: s }
            | Expr::Return { span: s, .. }
            | Expr::Reply { span: s, .. }
            | Expr::Yield { span: s }
            | Expr::Await { span: s, .. }
            | Expr::SharedExpr { span: s, .. }
            | Expr::Assign { span: s, .. }
            | Expr::CompoundAssign { span: s, .. }
            | Expr::While { span: s, .. }
            | Expr::For { span: s, .. }
            | Expr::Loop { span: s, .. }
            | Expr::Break { span: s, .. }
            | Expr::Continue { span: s } => *s,
            Expr::VecLit { span: s, .. }
            | Expr::MapLit { span: s, .. }
            | Expr::SetLit { span: s, .. } => *s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Neq => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Lte => "<=",
            BinOp::Gte => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

// ---------------------------------------------------------------------------
// String interpolation
// ---------------------------------------------------------------------------

/// An interpolated string literal: a sequence of literal text and expressions.
#[derive(Debug, Clone)]
pub struct StrLit {
    pub parts: Vec<StrPart>,
}

#[derive(Debug, Clone)]
pub enum StrPart {
    Literal(String),
    /// A parsed interpolation expression `"...{expr}..."`.
    Expr(Expr),
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// A `match` arm.
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    /// Usually a block or a bare expression; the `=>` rhs.
    pub body: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_`
    Wildcard(Span),
    /// A literal pattern: `1`, `"hi"`, `true`, `nil`.
    Lit(LitPattern, Span),
    /// Lowercase identifier — binds the matched value.
    Bind(String, Span),
    /// Uppercase identifier with optional args — enum variant pattern.
    /// `None`, `Some(x)`, `Rect(w, h)`.
    Variant {
        name: String,
        args: Vec<Pattern>,
        span: Span,
    },
    /// `{ x: px, y: py, .. }` — record destructuring.
    Struct {
        fields: Vec<(String, Pattern)>,
        rest: bool,
        span: Span,
    },
    /// `[a, b, ..rest]` — vector destructuring.
    Vec {
        pats: Vec<Pattern>,
        rest: bool,
        span: Span,
    },
    /// `p1 | p2 | p3` — or-pattern.
    Or(Vec<Pattern>, Span),
    /// `pat if guard`
    Guard(Box<Pattern>, Box<Expr>, Span),
}

#[derive(Debug, Clone)]
pub enum LitPattern {
    Int(num_bigint::BigInt),
    Decimal(bigdecimal::BigDecimal),
    Str(Vec<StrPart>),
    Bool(bool),
    Nil,
}

// ---------------------------------------------------------------------------
// Rescue clauses
// ---------------------------------------------------------------------------

/// `rescue TypeName as binding { body }` (both `TypeName` and `binding`
/// optional; omitting the type catches everything).
#[derive(Debug, Clone)]
pub struct RescueClause {
    pub type_name: Option<String>,
    pub bind: Option<String>,
    pub body: Box<Expr>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Type annotations
// ---------------------------------------------------------------------------

/// A type annotation. In the first version these are *not* statically checked
/// — they are documentation only. They are still parsed so that signatures are
/// stable for a future type-checking pass.
#[derive(Debug, Clone)]
pub enum TypeAnnot {
    /// `Int`, `Number`, `String`, `MyType`
    Name { name: String, span: Span },
    /// `Vec<Int>`, `Map<String, Int>`
    Generic {
        name: String,
        args: Vec<TypeAnnot>,
        span: Span,
    },
    /// `Int | String`
    Union { variants: Vec<TypeAnnot>, span: Span },
    /// `fn(Int, Int) -> Int`
    Fn {
        params: Vec<TypeAnnot>,
        ret: Box<TypeAnnot>,
        span: Span,
    },
}

impl TypeAnnot {
    pub fn span(&self) -> Span {
        match self {
            TypeAnnot::Name { span, .. }
            | TypeAnnot::Generic { span, .. }
            | TypeAnnot::Union { span, .. }
            | TypeAnnot::Fn { span, .. } => *span,
        }
    }
}
