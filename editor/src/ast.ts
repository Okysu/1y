// AST node definitions for the 1y language server.
//
// Mirrors `src/ast/mod.rs` from the Rust frontend. Spans are character
// offsets into the source (`start` inclusive, `end` exclusive). Every node
// carries a `span` so the LSP can map back to source ranges for hover,
// definition, and diagnostics.
//
// The parser (`parser.ts`) produces these nodes with error recovery: a
// whole file is always parsed to completion, recording syntax errors but
// still returning a (possibly partial) program.

// ---------------------------------------------------------------------------
// Span
// ---------------------------------------------------------------------------

export interface Span {
    start: number;
    end: number;
}

// ---------------------------------------------------------------------------
// Type annotations (user-written, not statically checked by the interpreter)
// ---------------------------------------------------------------------------

export type TypeAnnot =
    | { kind: "Name"; name: string; span: Span }
    | { kind: "Generic"; name: string; args: TypeAnnot[]; span: Span }
    | { kind: "Union"; variants: TypeAnnot[]; span: Span }
    | { kind: "Fn"; params: TypeAnnot[]; ret: TypeAnnot; span: Span };

/** Render a TypeAnnot back to source-like text, e.g. `Vec<Int>`, `Int | Nil`. */
export function annotToText(t: TypeAnnot): string {
    switch (t.kind) {
        case "Name": return t.name;
        case "Generic": return `${t.name}<${t.args.map(annotToText).join(", ")}>`;
        case "Union": return t.variants.map(annotToText).join(" | ");
        case "Fn": return `fn(${t.params.map(annotToText).join(", ")}) -> ${annotToText(t.ret)}`;
    }
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

export interface Param {
    name: string;
    typeAnnot: TypeAnnot | null;
    span: Span;
}

/** Render a param list, e.g. `(n: Int, acc: Int)`. */
export function paramsToText(params: Param[]): string {
    return "(" + params.map((p) => p.name + (p.typeAnnot ? ": " + annotToText(p.typeAnnot) : "")).join(", ") + ")";
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

export interface FuncDef {
    kind: "FuncDef";
    name: string;
    params: Param[];
    returnType: TypeAnnot | null;
    body: Expr;
    span: Span;
    /// Doc comment text (without `///` markers), accumulated from preceding
    /// `///` lines. Empty string if none. Filled in by the docstring pass.
    doc: string;
}

export interface TypeDef {
    kind: "TypeDef";
    name: string;
    typeParams: string[];
    fields: Array<{ name: string; type: TypeAnnot }>;
    span: Span;
    doc: string;
}

export interface VariantDef {
    name: string;
    fields: TypeAnnot[];
    span: Span;
}

export interface EnumDef {
    kind: "EnumDef";
    name: string;
    typeParams: string[];
    variants: VariantDef[];
    span: Span;
    doc: string;
}

export interface OnClause {
    kind: "OnClause";
    name: string;
    params: Param[];
    returnType: TypeAnnot | null;
    body: Expr;
    span: Span;
    doc: string;
}

export interface ActorDef {
    kind: "ActorDef";
    name: string;
    body: Stmt[];
    span: Span;
    doc: string;
}

export interface Import {
    kind: "Import";
    /// Dotted module path, e.g. `"net.socket"` (internal) or `"tls"` (stdlib).
    path: string;
    alias: string | null;
    lazy: boolean;
    span: Span;
}

export interface LetStmt {
    kind: "Let";
    name: string;
    typeAnnot: TypeAnnot | null;
    value: Expr;
    span: Span;
    doc: string;
}

export interface SharedDecl {
    kind: "SharedDecl";
    name: string;
    typeAnnot: TypeAnnot | null;
    value: Expr;
    span: Span;
    doc: string;
}

export interface StateDecl {
    kind: "StateDecl";
    name: string;
    typeAnnot: TypeAnnot | null;
    value: Expr;
    span: Span;
    doc: string;
}

export type Stmt =
    | LetStmt
    | SharedDecl
    | StateDecl
    | OnClause
    | FuncDef
    | TypeDef
    | EnumDef
    | ActorDef
    | Import
    | { kind: "Expr"; expr: Expr; span: Span }
    | { kind: "Semi"; expr: Expr; span: Span };

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

export type BinOp =
    | "Add" | "Sub" | "Mul" | "Div" | "Mod"
    | "Eq" | "Neq" | "Lt" | "Gt" | "Lte" | "Gte"
    | "And" | "Or";

export type UnaryOp = "Neg" | "Not";

export interface StrLit {
    parts: StrPart[];
}

export type StrPart =
    | { kind: "Literal"; text: string }
    | { kind: "Expr"; expr: Expr };

export interface MatchArm {
    pattern: Pattern;
    guard: Expr | null;
    body: Expr;
    span: Span;
}

export type Pattern =
    | { kind: "Wildcard"; span: Span }
    | { kind: "Lit"; lit: LitPattern; span: Span }
    | { kind: "Bind"; name: string; span: Span }
    | { kind: "Variant"; name: string; args: Pattern[]; span: Span }
    | { kind: "Struct"; name: string | null; fields: Array<{ name: string; pat: Pattern }>; rest: boolean; span: Span }
    | { kind: "Vec"; pats: Pattern[]; rest: boolean; span: Span }
    | { kind: "Or"; pats: Pattern[]; span: Span }
    | { kind: "Guard"; pat: Pattern; guard: Expr; span: Span };

export type LitPattern =
    | { kind: "Int"; text: string }
    | { kind: "Decimal"; text: string }
    | { kind: "Str"; parts: StrPart[] }
    | { kind: "Bool"; value: boolean }
    | { kind: "Nil" };

export interface RescueClause {
    typeName: string | null;
    bind: string | null;
    body: Expr;
    span: Span;
}

export type Expr =
    // Literals
    | { kind: "Int"; text: string; span: Span }
    | { kind: "Decimal"; text: string; span: Span }
    | { kind: "Str"; lit: StrLit; span: Span }
    | { kind: "Bool"; value: boolean; span: Span }
    | { kind: "Nil"; span: Span }
    | { kind: "Ident"; name: string; span: Span }
    // Collections
    | { kind: "VecLit"; items: Expr[]; span: Span }
    | { kind: "MapLit"; entries: Array<{ key: Expr; value: Expr }>; span: Span }
    | { kind: "SetLit"; items: Expr[]; span: Span }
    // Operators
    | { kind: "BinOp"; op: BinOp; lhs: Expr; rhs: Expr; span: Span }
    | { kind: "UnaryOp"; op: UnaryOp; expr: Expr; span: Span }
    | { kind: "Pipe"; lhs: Expr; rhs: Expr; span: Span }
    // Control flow
    | { kind: "If"; cond: Expr; then: Expr; else_: Expr | null; span: Span }
    | { kind: "Match"; scrutinee: Expr; arms: MatchArm[]; span: Span }
    // Functions
    | { kind: "Lambda"; params: Param[]; returnType: TypeAnnot | null; body: Expr; span: Span }
    | { kind: "Block"; stmts: Stmt[]; tail: Expr | null; span: Span }
    | { kind: "Paren"; expr: Expr; span: Span }
    // Calls / access
    | { kind: "Call"; callee: Expr; args: Expr[]; span: Span }
    | { kind: "MethodCall"; receiver: Expr; method: string; args: Expr[]; span: Span }
    | { kind: "Index"; target: Expr; index: Expr; span: Span }
    | { kind: "Field"; target: Expr; name: string; span: Span }
    // Actors
    | { kind: "ActorSend"; actor: Expr; msg: Expr; span: Span }
    | { kind: "ActorRequest"; actor: Expr; msg: Expr; span: Span }
    | { kind: "Spawn"; name: string; args: Expr[]; span: Span }
    // Exceptions / transactions
    | { kind: "Raise"; expr: Expr; span: Span }
    | { kind: "Try"; body: Expr; rescues: RescueClause[]; ensure: Expr | null; span: Span }
    | { kind: "Transact"; body: Expr; span: Span }
    | { kind: "Retry"; span: Span }
    // Control
    | { kind: "Return"; value: Expr | null; span: Span }
    | { kind: "Reply"; value: Expr; span: Span }
    | { kind: "Yield"; span: Span }
    | { kind: "Await"; expr: Expr; span: Span }
    | { kind: "SharedExpr"; expr: Expr; span: Span }
    // Assignment
    | { kind: "Assign"; target: Expr; value: Expr; span: Span }
    | { kind: "CompoundAssign"; op: BinOp; target: Expr; value: Expr; span: Span }
    // Loops
    | { kind: "While"; cond: Expr; body: Expr; span: Span }
    | { kind: "For"; var: string; iter: Expr; body: Expr; span: Span }
    | { kind: "Loop"; body: Expr; span: Span }
    | { kind: "Break"; value: Expr | null; span: Span }
    | { kind: "Continue"; span: Span };

/** Get the span of any expression node. */
export function exprSpan(e: Expr): Span {
    return e.span;
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

export interface Program {
    stmts: Stmt[];
    span: Span;
}

/** Parse error recorded during recovery. */
export interface ParseError {
    message: string;
    span: Span;
}

export interface ParseResult {
    program: Program;
    errors: ParseError[];
}
