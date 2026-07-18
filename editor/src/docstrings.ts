// Doc-comment extraction for the 1y language server.
//
// A `///` line doc comment documents the declaration that *immediately*
// follows it (no blank line in between). Multiple consecutive `///` lines
// form one doc block. Regular `//` comments and blank lines break the
// association.
//
// `extractDocStrings` returns a map from the declaration keyword's start
// offset to the doc text (with `///` markers stripped, lines joined by `\n`).
// `attachDocStrings` walks the AST and populates each declaration's `doc`
// field in place.
//
// The AST walkers (`walkStmt`, `walkExpr`) are exported for reuse by
// `symbols.ts` and `types.ts`.

import { tokenize, TokenType } from "./lexer";
import { Program, Stmt, Expr } from "./ast";

// Keywords that start a declaration and can carry a doc comment.
const DECL_KWS = new Set([
    "let", "fn", "type", "enum", "actor", "on", "import", "lazy",
    "shared", "state",
]);

/**
 * Build a map from declaration-keyword start offset → docstring text.
 *
 * Rules:
 *   - A `///` line documents the declaration keyword on the *next* line.
 *   - Consecutive `///` lines (no blank line between them) accumulate.
 *   - A regular `//` comment, a blank line, or any non-declaration token
 *     between the last `///` and the keyword discards the pending doc.
 */
export function extractDocStrings(src: string): Map<number, string> {
    const toks = tokenize(src);
    const result = new Map<number, string>();
    let buffer: string[] = [];
    let lastDocLine = -1;

    for (const tok of toks) {
        if (tok.type === TokenType.DocComment) {
            if (lastDocLine >= 0 && tok.line === lastDocLine + 1) {
                // Consecutive doc line — accumulate.
                buffer.push(stripDocMarker(tok.text));
            } else {
                // Gap (blank line) or first doc — start fresh.
                buffer = [stripDocMarker(tok.text)];
            }
            lastDocLine = tok.line;
            continue;
        }

        if (tok.type === TokenType.Comment) {
            // A regular `//` comment breaks the doc association.
            buffer = [];
            lastDocLine = -1;
            continue;
        }

        if (tok.type === TokenType.Eof) break;

        if (tok.type === TokenType.Keyword && DECL_KWS.has(tok.text)) {
            // Attach only if the last doc line is immediately above this kw.
            if (buffer.length > 0 && lastDocLine === tok.line - 1) {
                result.set(tok.start, buffer.join("\n"));
            }
            buffer = [];
            lastDocLine = -1;
            continue;
        }

        // Any other significant token breaks the pending doc.
        buffer = [];
        lastDocLine = -1;
    }

    return result;
}

/** Strip the leading `///` and one optional space; trim trailing whitespace. */
function stripDocMarker(text: string): string {
    let s = text.slice(3); // remove `///`
    if (s.startsWith(" ")) s = s.slice(1);
    return s.trimEnd();
}

/**
 * Walk the AST and populate each declaration's `doc` field from the
 * extracted doc-comment map. Mutates nodes in place.
 */
export function attachDocStrings(src: string, program: Program): void {
    const docs = extractDocStrings(src);
    for (const stmt of walkProgram(program)) {
        const doc = docs.get(stmt.span.start);
        if (doc === undefined) continue;
        switch (stmt.kind) {
            case "FuncDef":
            case "TypeDef":
            case "EnumDef":
            case "OnClause":
            case "ActorDef":
            case "Let":
            case "SharedDecl":
            case "StateDecl":
                (stmt as { doc: string }).doc = doc;
                break;
        }
    }
}

// ---------------------------------------------------------------------------
// AST walkers — yield every Stmt / Expr node so other passes (symbols, types)
// can traverse the tree uniformly.
// ---------------------------------------------------------------------------

/** Yield every Stmt in the program (top-level + nested). */
export function* walkProgram(program: Program): Generator<Stmt> {
    for (const s of program.stmts) yield* walkStmt(s);
}

/** Yield a stmt and all stmts/exprs nested inside it. */
export function* walkStmt(stmt: Stmt): Generator<Stmt> {
    yield stmt;
    switch (stmt.kind) {
        case "ActorDef":
            for (const s of stmt.body) yield* walkStmt(s);
            break;
        case "FuncDef":
            yield* walkExpr(stmt.body);
            break;
        case "OnClause":
            yield* walkExpr(stmt.body);
            break;
        case "Let":
            yield* walkExpr(stmt.value);
            break;
        case "SharedDecl":
            yield* walkExpr(stmt.value);
            break;
        case "StateDecl":
            yield* walkExpr(stmt.value);
            break;
        case "Expr":
            yield* walkExpr(stmt.expr);
            break;
        case "Semi":
            yield* walkExpr(stmt.expr);
            break;
        // TypeDef, EnumDef, Import — no nested stmts/exprs to walk.
    }
}

/** Yield all stmts/exprs nested inside an expression. Does NOT yield `expr`
 *  itself (callers usually want to inspect `expr` directly). */
export function* walkExpr(expr: Expr): Generator<Stmt> {
    switch (expr.kind) {
        case "Block":
            for (const s of expr.stmts) yield* walkStmt(s);
            if (expr.tail) yield* walkExpr(expr.tail);
            break;
        case "Lambda":
            yield* walkExpr(expr.body);
            break;
        case "If":
            yield* walkExpr(expr.cond);
            yield* walkExpr(expr.then);
            if (expr.else_) yield* walkExpr(expr.else_);
            break;
        case "Match":
            yield* walkExpr(expr.scrutinee);
            for (const arm of expr.arms) {
                if (arm.guard) yield* walkExpr(arm.guard);
                yield* walkExpr(arm.body);
            }
            break;
        case "BinOp":
        case "Pipe":
            yield* walkExpr(expr.lhs);
            yield* walkExpr(expr.rhs);
            break;
        case "UnaryOp":
        case "Paren":
        case "Raise":
        case "Await":
        case "SharedExpr":
            yield* walkExpr(expr.expr);
            break;
        case "Transact":
        case "Loop":
            yield* walkExpr(expr.body);
            break;
        case "Call":
            yield* walkExpr(expr.callee);
            for (const a of expr.args) yield* walkExpr(a);
            break;
        case "MethodCall":
            yield* walkExpr(expr.receiver);
            for (const a of expr.args) yield* walkExpr(a);
            break;
        case "Index":
            yield* walkExpr(expr.target);
            yield* walkExpr(expr.index);
            break;
        case "Field":
            yield* walkExpr(expr.target);
            break;
        case "VecLit":
        case "SetLit":
            for (const i of expr.items) yield* walkExpr(i);
            break;
        case "MapLit":
            for (const e of expr.entries) {
                yield* walkExpr(e.key);
                yield* walkExpr(e.value);
            }
            break;
        case "Assign":
        case "CompoundAssign":
            yield* walkExpr(expr.target);
            yield* walkExpr(expr.value);
            break;
        case "While":
            yield* walkExpr(expr.cond);
            yield* walkExpr(expr.body);
            break;
        case "For":
            yield* walkExpr(expr.iter);
            yield* walkExpr(expr.body);
            break;
        case "Break":
            if (expr.value) yield* walkExpr(expr.value);
            break;
        case "Return":
            if (expr.value) yield* walkExpr(expr.value);
            break;
        case "Reply":
            yield* walkExpr(expr.value);
            break;
        case "Try":
            yield* walkExpr(expr.body);
            for (const r of expr.rescues) yield* walkExpr(r.body);
            if (expr.ensure) yield* walkExpr(expr.ensure);
            break;
        case "ActorSend":
        case "ActorRequest":
            yield* walkExpr(expr.actor);
            yield* walkExpr(expr.msg);
            break;
        // Literals (Int, Decimal, Str, Bool, Nil, Ident), Spawn, Yield,
        // Retry, Continue — no nested exprs.
    }
}
