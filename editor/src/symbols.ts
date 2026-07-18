// Scope-aware symbol table for the 1y language server.
//
// Builds a flat list of declarations, each annotated with the scope range
// [scopeStart, scopeEnd) it belongs to and the position `visibleFrom` after
// which it becomes visible. This is sufficient for the core LSP queries:
//
//   - hover / go-to-definition: resolve the identifier at position P
//   - completion: collect all symbols visible at P
//   - document symbols: list all top-level declarations for the outline
//
// Scopes are created by: the file (global), function bodies, actor bodies,
// block expressions, lambdas, for-loops, and match arms. Lexical scoping
// is modeled by tracking a scope stack during AST traversal.

import { tokenize, TokenType, Token } from "./lexer";
import {
    Program, Stmt, Expr, Param, TypeAnnot, Span, Pattern, VariantDef,
} from "./ast";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export type SymbolKind =
    | "function" | "type" | "enum" | "actor" | "on"
    | "let" | "shared" | "state" | "param" | "lambda_param"
    | "import" | "for_var" | "match_bind" | "variant";

export interface SymbolInfo {
    name: string;
    kind: SymbolKind;
    /** Full declaration span (keyword → end). */
    declSpan: Span;
    /** Span of just the name identifier — jump target for go-to-definition. */
    nameSpan: Span;
    doc: string;
    typeAnnot: TypeAnnot | null;
    /** Function / handler params. */
    params?: Param[];
    /** Function / handler return type. */
    returnType?: TypeAnnot | null;
    /** Type definition fields. */
    fields?: Array<{ name: string; type: TypeAnnot }>;
    /** Enum variant argument types. */
    variantArgs?: TypeAnnot[];
    /** Enum definition variants. */
    variants?: VariantDef[];
    /** For `import` symbols: the module path (e.g. "json" or "net.socket"). */
    modulePath?: string;
    /** Scope range [start, end) this declaration belongs to. */
    scopeStart: number;
    scopeEnd: number;
    /** Position after which the name is visible (>= nameSpan.end). */
    visibleFrom: number;
}

export interface SymbolTable {
    /** All declarations in the file (flat). */
    decls: SymbolInfo[];
}

// ---------------------------------------------------------------------------
// Table construction
// ---------------------------------------------------------------------------

/**
 * Build a symbol table from source + parsed program. The source is needed to
 * resolve name spans (the AST stores full-declaration spans only).
 */
export function buildSymbolTable(src: string, program: Program): SymbolTable {
    const toks = tokenize(src).filter(
        (t) => t.type !== TokenType.Comment && t.type !== TokenType.DocComment,
    );
    // Index: token.start → position in `toks`, for O(1) name-token lookup.
    const tokIndex = new Map<number, number>();
    toks.forEach((t, i) => tokIndex.set(t.start, i));

    const decls: SymbolInfo[] = [];
    // Scope stack: each entry is the range of the current lexical scope.
    const scopeStack: Array<{ start: number; end: number }> = [
        { start: 0, end: Number.POSITIVE_INFINITY },
    ];
    const cur = () => scopeStack[scopeStack.length - 1];

    /** Find the span of the name identifier following the keyword at `kwStart`. */
    function nameSpanOf(kwStart: number): Span {
        const i = tokIndex.get(kwStart);
        if (i !== undefined && i + 1 < toks.length) {
            const nt = toks[i + 1];
            return { start: nt.start, end: nt.end };
        }
        return { start: kwStart, end: kwStart };
    }

    function add(
        name: string,
        kind: SymbolKind,
        declSpan: Span,
        nameSpan: Span,
        doc: string,
        typeAnnot: TypeAnnot | null,
        extra?: Partial<SymbolInfo>,
    ): void {
        const s = cur();
        decls.push({
            name, kind, declSpan, nameSpan, doc, typeAnnot,
            scopeStart: s.start,
            scopeEnd: s.end,
            visibleFrom: nameSpan.end,
            ...extra,
        });
    }

    function visitStmts(stmts: Stmt[]): void {
        for (const s of stmts) visitStmt(s);
    }

    function visitStmt(stmt: Stmt): void {
        switch (stmt.kind) {
            case "FuncDef": {
                // Functions are visible from the start of their enclosing
                // scope (hoisting), so mutually-recursive / forward
                // references resolve. `let`/`shared`/`state` keep strict
                // declaration-order visibility.
                const scopeStart = cur().start;
                add(stmt.name, "function", stmt.span, nameSpanOf(stmt.span.start),
                    stmt.doc, stmt.returnType,
                    { params: stmt.params, returnType: stmt.returnType, visibleFrom: scopeStart });
                scopeStack.push({ start: stmt.span.start, end: stmt.span.end });
                for (const p of stmt.params) add(p.name, "param", p.span, p.span, "", p.typeAnnot);
                visitExpr(stmt.body);
                scopeStack.pop();
                break;
            }

            case "OnClause": {
                const scopeStart = cur().start;
                add(stmt.name, "on", stmt.span, nameSpanOf(stmt.span.start),
                    stmt.doc, stmt.returnType,
                    { params: stmt.params, returnType: stmt.returnType, visibleFrom: scopeStart });
                scopeStack.push({ start: stmt.span.start, end: stmt.span.end });
                for (const p of stmt.params) add(p.name, "param", p.span, p.span, "", p.typeAnnot);
                visitExpr(stmt.body);
                scopeStack.pop();
                break;
            }

            case "ActorDef":
                add(stmt.name, "actor", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, null);
                scopeStack.push({ start: stmt.span.start, end: stmt.span.end });
                visitStmts(stmt.body);
                scopeStack.pop();
                break;

            case "TypeDef":
                add(stmt.name, "type", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, null,
                    { fields: stmt.fields });
                break;

            case "EnumDef":
                add(stmt.name, "enum", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, null,
                    { variants: stmt.variants });
                // Variants are visible from the enum's closing brace onward,
                // but for simplicity (and matching how they're used as
                // constructors) we register them in the current scope with
                // visibility from the variant's own span.
                for (const v of stmt.variants) {
                    add(v.name, "variant", v.span, nameSpanOf(v.span.start), "", null,
                        { variantArgs: v.fields });
                }
                break;

            case "Import": {
                const aliasName = stmt.alias || stmt.path.split(".").pop() || stmt.path;
                add(aliasName, "import", stmt.span, nameSpanOf(stmt.span.start), "", null,
                    { modulePath: stmt.path });
                break;
            }

            case "Let":
                add(stmt.name, "let", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, stmt.typeAnnot);
                visitExpr(stmt.value);
                break;

            case "SharedDecl":
                add(stmt.name, "shared", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, stmt.typeAnnot);
                visitExpr(stmt.value);
                break;

            case "StateDecl":
                add(stmt.name, "state", stmt.span, nameSpanOf(stmt.span.start), stmt.doc, stmt.typeAnnot);
                visitExpr(stmt.value);
                break;

            case "Expr":
                visitExpr(stmt.expr);
                break;

            case "Semi":
                visitExpr(stmt.expr);
                break;
        }
    }

    function visitExpr(expr: Expr): void {
        switch (expr.kind) {
            case "Block":
                scopeStack.push({ start: expr.span.start, end: expr.span.end });
                visitStmts(expr.stmts);
                if (expr.tail) visitExpr(expr.tail);
                scopeStack.pop();
                break;

            case "Lambda":
                scopeStack.push({ start: expr.span.start, end: expr.span.end });
                for (const p of expr.params) add(p.name, "lambda_param", p.span, p.span, "", p.typeAnnot);
                visitExpr(expr.body);
                scopeStack.pop();
                break;

            case "If":
                visitExpr(expr.cond);
                visitExpr(expr.then);
                if (expr.else_) visitExpr(expr.else_);
                break;

            case "Match":
                visitExpr(expr.scrutinee);
                for (const arm of expr.arms) {
                    scopeStack.push({ start: arm.span.start, end: arm.span.end });
                    registerPatternBindings(arm.pattern);
                    if (arm.guard) visitExpr(arm.guard);
                    visitExpr(arm.body);
                    scopeStack.pop();
                }
                break;

            case "For":
                scopeStack.push({ start: expr.span.start, end: expr.span.end });
                add(expr.var, "for_var", expr.span, nameSpanOf(expr.span.start), "", null);
                visitExpr(expr.iter);
                visitExpr(expr.body);
                scopeStack.pop();
                break;

            case "BinOp":
            case "Pipe":
                visitExpr(expr.lhs);
                visitExpr(expr.rhs);
                break;

            case "UnaryOp":
            case "Paren":
            case "Raise":
            case "Await":
            case "SharedExpr":
                visitExpr(expr.expr);
                break;

            case "Transact":
            case "Loop":
                visitExpr(expr.body);
                break;

            case "Call":
                visitExpr(expr.callee);
                for (const a of expr.args) visitExpr(a);
                break;

            case "MethodCall":
                visitExpr(expr.receiver);
                for (const a of expr.args) visitExpr(a);
                break;

            case "Index":
                visitExpr(expr.target);
                visitExpr(expr.index);
                break;

            case "Field":
                visitExpr(expr.target);
                break;

            case "VecLit":
            case "SetLit":
                for (const i of expr.items) visitExpr(i);
                break;

            case "MapLit":
                for (const e of expr.entries) {
                    visitExpr(e.key);
                    visitExpr(e.value);
                }
                break;

            case "Assign":
            case "CompoundAssign":
                visitExpr(expr.target);
                visitExpr(expr.value);
                break;

            case "While":
                visitExpr(expr.cond);
                visitExpr(expr.body);
                break;

            case "Break":
                if (expr.value) visitExpr(expr.value);
                break;

            case "Return":
                if (expr.value) visitExpr(expr.value);
                break;

            case "Reply":
                visitExpr(expr.value);
                break;

            case "Try":
                visitExpr(expr.body);
                for (const r of expr.rescues) visitExpr(r.body);
                if (expr.ensure) visitExpr(expr.ensure);
                break;

            case "ActorSend":
            case "ActorRequest":
                visitExpr(expr.actor);
                visitExpr(expr.msg);
                break;

            // Literals, Spawn, Yield, Retry, Continue — no sub-exprs to scope.
        }
    }

    function registerPatternBindings(pat: Pattern): void {
        switch (pat.kind) {
            case "Bind":
                add(pat.name, "match_bind", pat.span, pat.span, "", null);
                break;
            case "Variant":
                for (const sub of pat.args) registerPatternBindings(sub);
                break;
            case "Struct":
                for (const f of pat.fields) registerPatternBindings(f.pat);
                break;
            case "Vec":
                for (const sub of pat.pats) registerPatternBindings(sub);
                break;
            case "Or":
                for (const sub of pat.pats) registerPatternBindings(sub);
                break;
            case "Guard":
                registerPatternBindings(pat.pat);
                break;
            // Wildcard, Lit — no bindings.
        }
    }

    visitStmts(program.stmts);
    return { decls };
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/**
 * Resolve a name at position `pos`. Returns the nearest visible declaration,
 * preferring inner scopes and later declarations within the same scope.
 */
export function resolveAt(table: SymbolTable, pos: number, name: string): SymbolInfo | null {
    let best: SymbolInfo | null = null;
    for (const d of table.decls) {
        if (d.name !== name) continue;
        if (pos < d.scopeStart || pos >= d.scopeEnd) continue;
        if (pos < d.visibleFrom) continue;
        if (
            !best ||
            d.scopeStart > best.scopeStart ||
            (d.scopeStart === best.scopeStart && d.visibleFrom > best.visibleFrom)
        ) {
            best = d;
        }
    }
    return best;
}

/**
 * Collect all symbols visible at position `pos` (for completion). Excludes
 * declarations that appear after `pos` in the same scope.
 */
export function visibleAt(table: SymbolTable, pos: number): SymbolInfo[] {
    return table.decls.filter(
        (d) => pos >= d.scopeStart && pos < d.scopeEnd && pos >= d.visibleFrom,
    );
}

/**
 * Find the identifier (or keyword-used-as-name) token at position `pos`.
 * Returns the token text and span, or null if `pos` is not on an identifier.
 */
export function identAt(toks: Token[], pos: number): { name: string; span: Span } | null {
    for (const t of toks) {
        if (pos >= t.start && pos < t.end) {
            if (t.type === TokenType.Ident || t.type === TokenType.Keyword) {
                return { name: t.text, span: { start: t.start, end: t.end } };
            }
            return null;
        }
    }
    return null;
}

/** Find the declaration whose nameSpan contains `pos` (for definition jump
 *  from the declaration name itself, or for document highlight). */
export function declAt(table: SymbolTable, pos: number): SymbolInfo | null {
    for (const d of table.decls) {
        if (pos >= d.nameSpan.start && pos < d.nameSpan.end) return d;
    }
    return null;
}
