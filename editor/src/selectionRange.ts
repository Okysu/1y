// Selection ranges for the 1y language server.
//
// Drives `textDocument/selectionRange`: given a cursor position, return a
// chain of progressively larger enclosing ranges so the editor's "expand
// selection" (Shift+Alt+→) walks outward through the AST. The innermost
// range is the identifier/token under the cursor; each parent link widens
// to the enclosing expression, statement, block, function, etc.

import { Program, Stmt, Expr, Span } from "./ast";

export interface SelectionRangeNode {
    range: Span;
    parent?: SelectionRangeNode;
}

/**
 * Build the selection-range chain for the node at `offset`. Returns the
 * innermost node; walk `.parent` to get outer ranges.
 */
export function buildSelectionRange(
    program: Program,
    offset: number,
): SelectionRangeNode | null {
    // Collect all AST node spans that contain `offset`, sorted by size
    // (smallest first). The chain is built innermost → outermost.
    const spans: Span[] = [];
    collectContainingSpans(program, offset, spans);
    // Sort by size ascending; ties broken by start (later start = more nested).
    spans.sort((a, b) => (a.end - a.start) - (b.end - b.start) || b.start - a.start);

    if (spans.length === 0) return null;

    // spans is smallest-first; build innermost → outermost chain so each
    // node's `.parent` is the next-larger enclosing range.
    let inner: SelectionRangeNode | undefined;
    for (const s of spans) {
        inner = { range: s, parent: inner };
    }
    return inner ?? null;
}

/** Collect every AST node span that strictly contains `offset`. */
function collectContainingSpans(program: Program, offset: number, out: Span[]): void {
    for (const s of program.stmts) {
        if (contains(s.span, offset)) {
            out.push(s.span);
            walkStmt(s, offset, out);
        }
    }
}

function contains(span: Span, offset: number): boolean {
    return offset >= span.start && offset < span.end;
}

function walkStmt(stmt: Stmt, offset: number, out: Span[]): void {
    switch (stmt.kind) {
        case "FuncDef":
        case "OnClause":
            if (contains(stmt.body.span, offset)) {
                out.push(stmt.body.span);
                walkExpr(stmt.body, offset, out);
            }
            break;
        case "ActorDef":
            for (const s of stmt.body) {
                if (contains(s.span, offset)) {
                    out.push(s.span);
                    walkStmt(s, offset, out);
                }
            }
            break;
        case "Let":
        case "SharedDecl":
        case "StateDecl":
            if (contains(stmt.value.span, offset)) {
                out.push(stmt.value.span);
                walkExpr(stmt.value, offset, out);
            }
            break;
        case "Expr":
        case "Semi":
            if (contains(stmt.expr.span, offset)) {
                out.push(stmt.expr.span);
                walkExpr(stmt.expr, offset, out);
            }
            break;
        case "TypeDef":
        case "EnumDef":
        case "Import":
            break;
    }
}

function walkExpr(expr: Expr, offset: number, out: Span[]): void {
    if (!expr || typeof expr !== "object" || !expr.kind) return;
    switch (expr.kind) {
        case "Block":
            for (const s of expr.stmts || []) {
                if (contains(s.span, offset)) { out.push(s.span); walkStmt(s, offset, out); }
            }
            if (expr.tail && contains(expr.tail.span, offset)) {
                out.push(expr.tail.span);
                walkExpr(expr.tail, offset, out);
            }
            break;
        case "Lambda":
            if (contains(expr.body.span, offset)) {
                out.push(expr.body.span);
                walkExpr(expr.body, offset, out);
            }
            break;
        case "If":
            if (contains(expr.cond.span, offset)) { out.push(expr.cond.span); walkExpr(expr.cond, offset, out); }
            if (contains(expr.then.span, offset)) { out.push(expr.then.span); walkExpr(expr.then, offset, out); }
            if (expr.else_ && contains(expr.else_.span, offset)) { out.push(expr.else_.span); walkExpr(expr.else_, offset, out); }
            break;
        case "Match":
            if (contains(expr.scrutinee.span, offset)) { out.push(expr.scrutinee.span); walkExpr(expr.scrutinee, offset, out); }
            for (const arm of expr.arms || []) {
                if (contains(arm.body.span, offset)) { out.push(arm.body.span); walkExpr(arm.body, offset, out); }
            }
            break;
        case "While":
            if (contains(expr.cond.span, offset)) { out.push(expr.cond.span); walkExpr(expr.cond, offset, out); }
            if (contains(expr.body.span, offset)) { out.push(expr.body.span); walkExpr(expr.body, offset, out); }
            break;
        case "For":
            if (contains(expr.iter.span, offset)) { out.push(expr.iter.span); walkExpr(expr.iter, offset, out); }
            if (contains(expr.body.span, offset)) { out.push(expr.body.span); walkExpr(expr.body, offset, out); }
            break;
        case "Loop":
            if (contains(expr.body.span, offset)) { out.push(expr.body.span); walkExpr(expr.body, offset, out); }
            break;
        case "BinOp":
            if (contains(expr.lhs.span, offset)) { out.push(expr.lhs.span); walkExpr(expr.lhs, offset, out); }
            if (contains(expr.rhs.span, offset)) { out.push(expr.rhs.span); walkExpr(expr.rhs, offset, out); }
            break;
        case "Pipe":
            if (contains(expr.lhs.span, offset)) { out.push(expr.lhs.span); walkExpr(expr.lhs, offset, out); }
            if (contains(expr.rhs.span, offset)) { out.push(expr.rhs.span); walkExpr(expr.rhs, offset, out); }
            break;
        case "UnaryOp":
        case "Paren":
        case "Raise":
        case "Await":
        case "SharedExpr":
            if (contains(expr.expr.span, offset)) { out.push(expr.expr.span); walkExpr(expr.expr, offset, out); }
            break;
        case "Transact":
            if (contains(expr.body.span, offset)) { out.push(expr.body.span); walkExpr(expr.body, offset, out); }
            break;
        case "Try":
            if (contains(expr.body.span, offset)) { out.push(expr.body.span); walkExpr(expr.body, offset, out); }
            for (const r of expr.rescues || []) {
                if (contains(r.body.span, offset)) { out.push(r.body.span); walkExpr(r.body, offset, out); }
            }
            if (expr.ensure && contains(expr.ensure.span, offset)) { out.push(expr.ensure.span); walkExpr(expr.ensure, offset, out); }
            break;
        case "Call":
            if (contains(expr.callee.span, offset)) { out.push(expr.callee.span); walkExpr(expr.callee, offset, out); }
            for (const a of expr.args || []) {
                if (contains(a.span, offset)) { out.push(a.span); walkExpr(a, offset, out); }
            }
            break;
        case "MethodCall":
            if (contains(expr.receiver.span, offset)) { out.push(expr.receiver.span); walkExpr(expr.receiver, offset, out); }
            for (const a of expr.args || []) {
                if (contains(a.span, offset)) { out.push(a.span); walkExpr(a, offset, out); }
            }
            break;
        case "Index":
            if (contains(expr.target.span, offset)) { out.push(expr.target.span); walkExpr(expr.target, offset, out); }
            if (contains(expr.index.span, offset)) { out.push(expr.index.span); walkExpr(expr.index, offset, out); }
            break;
        case "Field":
            if (contains(expr.target.span, offset)) { out.push(expr.target.span); walkExpr(expr.target, offset, out); }
            break;
        case "VecLit":
            for (const it of expr.items || []) {
                if (contains(it.span, offset)) { out.push(it.span); walkExpr(it, offset, out); }
            }
            break;
        case "MapLit":
            for (const e of expr.entries || []) {
                if (contains(e.key.span, offset)) { out.push(e.key.span); walkExpr(e.key, offset, out); }
                if (contains(e.value.span, offset)) { out.push(e.value.span); walkExpr(e.value, offset, out); }
            }
            break;
        case "SetLit":
            for (const it of expr.items || []) {
                if (contains(it.span, offset)) { out.push(it.span); walkExpr(it, offset, out); }
            }
            break;
        case "Assign":
        case "CompoundAssign":
            if (contains(expr.target.span, offset)) { out.push(expr.target.span); walkExpr(expr.target, offset, out); }
            if (contains(expr.value.span, offset)) { out.push(expr.value.span); walkExpr(expr.value, offset, out); }
            break;
        case "Return":
            if (expr.value && contains(expr.value.span, offset)) { out.push(expr.value.span); walkExpr(expr.value, offset, out); }
            break;
        case "Reply":
            if (contains(expr.value.span, offset)) { out.push(expr.value.span); walkExpr(expr.value, offset, out); }
            break;
        // Literals, idents, Break, Continue, Yield, Retry, Str — no children.
    }
}
