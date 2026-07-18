// Folding ranges for the 1y language server.
//
// Drives `textDocument/foldingRange`: produces foldable regions for function
// bodies, actor bodies, block expressions, multi-line collection literals,
// and multi-line `///` doc comment blocks. Line-based (LSP folding works on
// line ranges, not character offsets).

import { Program, Stmt, Expr, Span } from "./ast";

export interface FoldingRangeInfo {
    startLine: number;   // 0-based
    endLine: number;     // 0-based, inclusive
    kind?: "comment" | "imports" | "region";
}

/** Build foldable ranges from the AST. `lineOf(offset)` maps char→line. */
export function buildFoldingRanges(
    program: Program,
    lineOf: (offset: number) => number,
): FoldingRangeInfo[] {
    const ranges: FoldingRangeInfo[] = [];
    const push = (span: Span, kind?: FoldingRangeInfo["kind"]) => {
        const s = lineOf(span.start);
        const e = lineOf(span.end - 1); // end is exclusive → last char line
        if (e > s) ranges.push({ startLine: s, endLine: e, kind });
    };

    for (const s of program.stmts) walkStmt(s, push);
    return ranges;
}

function walkStmt(stmt: Stmt, push: (span: Span, kind?: FoldingRangeInfo["kind"]) => void): void {
    switch (stmt.kind) {
        case "FuncDef":
        case "OnClause":
            // Fold the whole declaration when the body spans multiple lines.
            push(stmt.span);
            walkExpr(stmt.body, push);
            break;
        case "ActorDef":
            push(stmt.span);
            for (const s of stmt.body) walkStmt(s, push);
            break;
        case "TypeDef":
        case "EnumDef":
            push(stmt.span);
            break;
        case "Let":
        case "SharedDecl":
        case "StateDecl":
            walkExpr(stmt.value, push);
            break;
        case "Expr":
        case "Semi":
            walkExpr(stmt.expr, push);
            break;
        case "Import":
            break;
    }
}

function walkExpr(expr: Expr, push: (span: Span) => void): void {
    if (!expr || typeof expr !== "object" || !expr.kind) return;
    switch (expr.kind) {
        case "Block":
            push(expr.span);
            for (const s of expr.stmts || []) walkStmt(s, push);
            if (expr.tail) walkExpr(expr.tail, push);
            break;
        case "Lambda":
            push(expr.span);
            walkExpr(expr.body, push);
            break;
        case "If":
            walkExpr(expr.cond, push);
            walkExpr(expr.then, push);
            if (expr.else_) walkExpr(expr.else_, push);
            break;
        case "Match":
            walkExpr(expr.scrutinee, push);
            for (const arm of expr.arms || []) walkExpr(arm.body, push);
            break;
        case "While":
        case "For":
        case "Loop":
            walkExpr(expr.body, push);
            break;
        case "Try":
            push(expr.span);
            walkExpr(expr.body, push);
            for (const r of expr.rescues || []) walkExpr(r.body, push);
            if (expr.ensure) walkExpr(expr.ensure, push);
            break;
        case "Transact":
            walkExpr(expr.body, push);
            break;
        case "VecLit":
        case "MapLit":
        case "SetLit":
            push(expr.span);
            break;
        case "Call":
            walkExpr(expr.callee, push);
            for (const a of expr.args || []) walkExpr(a, push);
            break;
        case "MethodCall":
            walkExpr(expr.receiver, push);
            for (const a of expr.args || []) walkExpr(a, push);
            break;
        case "BinOp":
            walkExpr(expr.lhs, push);
            walkExpr(expr.rhs, push);
            break;
        case "Pipe":
            walkExpr(expr.lhs, push);
            walkExpr(expr.rhs, push);
            break;
        // Other nodes have no foldable substructure.
    }
}

/**
 * Build foldable ranges for consecutive `///` doc comment blocks and
 * consecutive `import` statements. Operates on the raw token stream since
 * the parser strips comments.
 */
export function buildCommentAndImportFolds(
    src: string,
    lineOf: (offset: number) => number,
): FoldingRangeInfo[] {
    const ranges: FoldingRangeInfo[] = [];
    const lines = src.split(/\r?\n/);

    // 1. Consecutive `///` doc comment lines.
    let docStart = -1;
    for (let i = 0; i < lines.length; i++) {
        const isDoc = /^\s*\/\//.test(lines[i]);
        if (isDoc) {
            if (docStart < 0) docStart = i;
        } else {
            if (docStart >= 0 && i - 1 > docStart) {
                ranges.push({ startLine: docStart, endLine: i - 1, kind: "comment" });
            }
            docStart = -1;
        }
    }
    if (docStart >= 0 && lines.length - 1 > docStart) {
        ranges.push({ startLine: docStart, endLine: lines.length - 1, kind: "comment" });
    }

    // 2. Consecutive `import` statements at the top of the file.
    let impStart = -1;
    for (let i = 0; i < lines.length; i++) {
        const isImp = /^\s*(lazy\s+)?import\s/.test(lines[i]);
        if (isImp) {
            if (impStart < 0) impStart = i;
        } else {
            if (impStart >= 0 && i - 1 > impStart) {
                ranges.push({ startLine: impStart, endLine: i - 1, kind: "imports" });
            }
            // Only fold the *first* contiguous import block.
            if (impStart >= 0) break;
            // Skip leading blank/comment lines before the first import.
            if (lines[i].trim() !== "" && !/^\s*\/\//.test(lines[i])) {
                // Non-import, non-blank, non-comment — stop scanning.
                break;
            }
        }
    }

    return ranges;
}
