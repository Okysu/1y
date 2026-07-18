// Recursive-descent + Pratt parser for the 1y language server.
//
// Produces the AST defined in `ast.ts`. Mirrors the precedence and grammar
// of the Rust frontend (`src/parser/mod.rs`) so the editor experience stays
// consistent with the authoritative parser.
//
// Error recovery: on a syntax error we record a ParseError, then resynchronize
// at the next statement boundary (`;`, `}`, or a declaration-starting
// keyword). A whole file is always parsed to completion, returning a (possibly
// partial) program plus a list of errors.
//
// Operator precedence (binding powers, left/right):
//   | 1,2  |>                          (left)
//   | 3,4  `!` `?`  actor send/request (left)
//   | 5,6  `or` `||`                   (left)
//   | 7,8  `and` `&&`                  (left)
//   | 9,10 `==` `!=` `<` `>` `<=` `>=` (left)
//   | 11,12 `+` `-`                    (left)
//   | 13,14 `*` `/` `%`                (left)
//   | 15   unary `-` `not`            (prefix)
//   | postfix `(` `[` `.`             (call/index/field/method)
// Assignment (`=` / `+=` ...) is right-associative at min_bp 0.

import { tokenize, Token, TokenType } from "./lexer";
import {
    Program, Stmt, Expr, Param, TypeAnnot, Span, ParseResult, ParseError,
    FuncDef, TypeDef, EnumDef, ActorDef, OnClause, Import,
    MatchArm, Pattern, LitPattern, RescueClause, StrLit, StrPart, BinOp, UnaryOp,
} from "./ast";

// Declaration-starting keywords (used for brace disambiguation & recovery).
const DECL_KEYWORDS = new Set([
    "let", "shared", "state", "fn", "type", "enum", "actor", "on",
    "import", "lazy",
]);

export function parse(src: string): ParseResult {
    const p = new Parser(src);
    return p.parseProgram();
}

class Parser {
    private toks: Token[];
    private pos = 0;
    private errors: ParseError[] = [];

    constructor(src: string) {
        // The lexer emits Comment/DocComment tokens, but the parser is
        // statement-structure-driven and never matches them as primaries.
        // Filter them out of the working stream so they don't trigger
        // spurious "unexpected token" errors. Doc comments are extracted
        // separately by `docstrings.ts` (which calls `tokenize` directly).
        this.toks = tokenize(src).filter(
            (t) => t.type !== TokenType.Comment && t.type !== TokenType.DocComment,
        );
    }

    parseProgram(): ParseResult {
        const stmts = this.parseStmtList(true);
        const span: Span = stmts.length
            ? { start: this.firstStart(stmts), end: this.lastEnd(stmts) }
            : { start: 0, end: 0 };
        return { program: { stmts, span }, errors: this.errors };
    }

    // -----------------------------------------------------------------------
    // Token helpers
    // -----------------------------------------------------------------------

    private peek(off = 0): Token {
        return this.toks[Math.min(this.pos + off, this.toks.length - 1)];
    }

    private bump(): Token {
        const t = this.toks[this.pos];
        if (t.type !== TokenType.Eof) this.pos++;
        return t;
    }

    private is(type: TokenType, off = 0): boolean {
        return this.peek(off).type === type;
    }

    private isKw(kw: string, off = 0): boolean {
        const t = this.peek(off);
        return t.type === TokenType.Keyword && t.text === kw;
    }

    private eat(type: TokenType): boolean {
        if (this.is(type)) { this.bump(); return true; }
        return false;
    }

    private eatKw(kw: string): boolean {
        if (this.isKw(kw)) { this.bump(); return true; }
        return false;
    }

    private expect(type: TokenType, msg: string): Token {
        if (this.is(type)) return this.bump();
        this.error(this.spanHere(), `expected ${msg}, found ${this.describe(this.peek())}`);
        // Return a dummy token at current position so parsing can continue.
        return this.peek();
    }

    private expectIdent(what: string): string {
        const t = this.peek();
        if (t.type === TokenType.Ident || (t.type === TokenType.Keyword && !isReservedKeyword(t.text))) {
            this.bump();
            return t.text;
        }
        this.error(this.spanHere(), `expected ${what}, found ${this.describe(t)}`);
        return "<error>";
    }

    private spanHere(): Span {
        const t = this.peek();
        return { start: t.start, end: t.end };
    }

    private prevSpan(): Span {
        const t = this.toks[Math.max(0, this.pos - 1)];
        return { start: t.start, end: t.end };
    }

    private union(a: Span, b: Span): Span {
        return { start: Math.min(a.start, b.start), end: Math.max(a.end, b.end) };
    }

    private error(span: Span, message: string): void {
        this.errors.push({ message, span });
    }

    private describe(t: Token): string {
        if (t.type === TokenType.Eof) return "end of file";
        if (t.text) return `'${t.text}'`;
        return TokenType[t.type];
    }

    // -----------------------------------------------------------------------
    // Statements
    // -----------------------------------------------------------------------

    private parseStmtList(_topLevel: boolean): Stmt[] {
        const stmts: Stmt[] = [];
        while (!this.is(TokenType.Eof) && !this.is(TokenType.RBrace)) {
            // Skip stray semicolons — e.g. after `let x = 1;` the declaration
            // parser does not consume the trailing `;`, so a leading `;` here
            // is an empty statement and must not be parsed as an expression.
            while (this.is(TokenType.Semicolon)) this.bump();
            if (this.is(TokenType.Eof) || this.is(TokenType.RBrace)) break;
            const before = this.pos;
            const s = this.parseStmt();
            if (s) stmts.push(s);
            // Guard against infinite loops on unrecoverable input.
            if (this.pos === before) this.bump();
        }
        return stmts;
    }

    private parseStmt(): Stmt | null {
        const start = this.spanHere();
        try {
            if (this.isKw("let")) return this.parseLet(start);
            if (this.isKw("shared")) {
                // `shared name = ...` is a decl; `shared expr` is an expression.
                if (this.peek(1).type === TokenType.Ident
                    && (this.peek(2).type === TokenType.Assign || this.peek(2).type === TokenType.Colon)) {
                    return this.parseShared(start);
                }
                return this.parseExprStmt(start);
            }
            if (this.isKw("fn")) {
                // `fn name(...)` decl vs `fn(...)` lambda expr statement.
                if (this.peek(1).type === TokenType.Ident) return this.parseFuncDef(start);
                return this.parseExprStmt(start);
            }
            if (this.isKw("type")) return this.parseTypeDef(start);
            if (this.isKw("enum")) return this.parseEnumDef(start);
            if (this.isKw("actor")) return this.parseActorDef(start);
            if (this.isKw("import")) return this.parseImport(start, false);
            if (this.isKw("lazy")) return this.parseImport(start, true);
            if (this.isKw("state")) return this.parseStateDecl(start);
            if (this.isKw("on")) return this.parseOnClause(start);
            return this.parseExprStmt(start);
        } catch (e) {
            // Shouldn't happen — we use error recording, not exceptions — but
            // guard anyway so one bad node never kills the whole parse.
            this.synchronize();
            return null;
        }
    }

    private parseExprStmt(start: Span): Stmt {
        const expr = this.parseExpr(0);
        const semi = this.eat(TokenType.Semicolon);
        const span = this.union(start, this.prevSpan());
        return semi ? { kind: "Semi", expr, span } : { kind: "Expr", expr, span };
    }

    private parseLet(start: Span): Stmt {
        this.bump(); // `let`
        const name = this.expectIdent("let binding name");
        const typeAnnot = this.maybeTypeAnnot();
        this.expect(TokenType.Assign, "`=`");
        const value = this.parseExpr(0);
        return { kind: "Let", name, typeAnnot, value, span: this.union(start, this.prevSpan()), doc: "" };
    }

    private parseShared(start: Span): Stmt {
        this.bump(); // `shared`
        const name = this.expectIdent("shared binding name");
        const typeAnnot = this.maybeTypeAnnot();
        this.expect(TokenType.Assign, "`=`");
        const value = this.parseExpr(0);
        return { kind: "SharedDecl", name, typeAnnot, value, span: this.union(start, this.prevSpan()), doc: "" };
    }

    private parseStateDecl(start: Span): Stmt {
        this.bump(); // `state`
        const name = this.expectIdent("state binding name");
        const typeAnnot = this.maybeTypeAnnot();
        this.expect(TokenType.Assign, "`=`");
        const value = this.parseExpr(0);
        return { kind: "StateDecl", name, typeAnnot, value, span: this.union(start, this.prevSpan()), doc: "" };
    }

    private parseFuncDef(start: Span): Stmt {
        this.bump(); // `fn`
        const name = this.expectIdent("function name");
        const params = this.parseParams();
        const returnType = this.eat(TokenType.Arrow) ? this.parseType() : null;
        const body = this.parseExpr(0);
        const fd: FuncDef = {
            kind: "FuncDef", name, params, returnType, body,
            span: this.union(start, this.prevSpan()), doc: "",
        };
        return fd;
    }

    private parseOnClause(start: Span): Stmt {
        this.bump(); // `on`
        const name = this.expectIdent("handler name");
        const params = this.parseParams();
        const returnType = this.eat(TokenType.Arrow) ? this.parseType() : null;
        const body = this.parseExpr(0);
        const oc: OnClause = {
            kind: "OnClause", name, params, returnType, body,
            span: this.union(start, this.prevSpan()), doc: "",
        };
        return oc;
    }

    private parseTypeDef(start: Span): Stmt {
        this.bump(); // `type`
        const name = this.expectIdent("type name");
        const typeParams = this.parseTypeParams();
        this.expect(TokenType.Assign, "`=`");
        this.expect(TokenType.LBrace, "`{`");
        const fields: Array<{ name: string; type: TypeAnnot }> = [];
        while (!this.is(TokenType.RBrace) && !this.is(TokenType.Eof)) {
            const fname = this.expectIdent("field name");
            this.expect(TokenType.Colon, "`:`");
            const ft = this.parseType();
            fields.push({ name: fname, type: ft });
            if (!this.eat(TokenType.Comma)) break;
        }
        this.expect(TokenType.RBrace, "`}`");
        const td: TypeDef = {
            kind: "TypeDef", name, typeParams, fields,
            span: this.union(start, this.prevSpan()), doc: "",
        };
        return td;
    }

    private parseEnumDef(start: Span): Stmt {
        this.bump(); // `enum`
        const name = this.expectIdent("enum name");
        const typeParams = this.parseTypeParams();
        this.expect(TokenType.LBrace, "`{`");
        const variants: EnumDef["variants"] = [];
        while (!this.is(TokenType.RBrace) && !this.is(TokenType.Eof)) {
            const vstart = this.spanHere();
            const vname = this.expectIdent("variant name");
            const vfields: TypeAnnot[] = [];
            if (this.eat(TokenType.LParen)) {
                if (!this.is(TokenType.RParen)) {
                    do {
                        vfields.push(this.parseType());
                    } while (this.eat(TokenType.Comma));
                }
                this.expect(TokenType.RParen, "`)`");
            }
            variants.push({ name: vname, fields: vfields, span: this.union(vstart, this.prevSpan()) });
            if (!this.eat(TokenType.Comma)) break;
        }
        this.expect(TokenType.RBrace, "`}`");
        const ed: EnumDef = {
            kind: "EnumDef", name, typeParams, variants,
            span: this.union(start, this.prevSpan()), doc: "",
        };
        return ed;
    }

    private parseActorDef(start: Span): Stmt {
        this.bump(); // `actor`
        const name = this.expectIdent("actor name");
        this.expect(TokenType.LBrace, "`{`");
        const body: Stmt[] = [];
        while (!this.is(TokenType.RBrace) && !this.is(TokenType.Eof)) {
            const before = this.pos;
            const s = this.parseStmt();
            if (s) body.push(s);
            if (this.pos === before) this.bump();
        }
        this.expect(TokenType.RBrace, "`}`");
        const ad: ActorDef = {
            kind: "ActorDef", name, body,
            span: this.union(start, this.prevSpan()), doc: "",
        };
        return ad;
    }

    private parseImport(start: Span, lazy: boolean): Stmt {
        if (lazy) this.bump(); // `lazy`
        this.bump(); // `import`
        const path = this.parseDottedPath();
        let alias: string | null = null;
        if (this.eatKw("as")) alias = this.expectIdent("alias name");
        const imp: Import = {
            kind: "Import", path, alias, lazy,
            span: this.union(start, this.prevSpan()),
        };
        return imp;
    }

    private parseDottedPath(): string {
        let path = this.expectIdent("module path");
        while (this.eat(TokenType.Dot)) {
            path += "." + this.expectIdent("module name");
        }
        return path;
    }

    // -----------------------------------------------------------------------
    // Parameters & type annotations
    // -----------------------------------------------------------------------

    private parseParams(): Param[] {
        this.expect(TokenType.LParen, "`(`");
        const params: Param[] = [];
        if (!this.is(TokenType.RParen)) {
            do {
                const pstart = this.spanHere();
                const name = this.expectIdent("parameter name");
                const typeAnnot = this.maybeTypeAnnot();
                params.push({ name, typeAnnot, span: this.union(pstart, this.prevSpan()) });
            } while (this.eat(TokenType.Comma));
        }
        this.expect(TokenType.RParen, "`)`");
        return params;
    }

    private maybeTypeAnnot(): TypeAnnot | null {
        if (this.eat(TokenType.Colon)) return this.parseType();
        return null;
    }

    private parseTypeParams(): string[] {
        const tps: string[] = [];
        if (this.eat(TokenType.Lt)) {
            if (!this.is(TokenType.Gt)) {
                do {
                    tps.push(this.expectIdent("type parameter"));
                } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.Gt, "`>`");
        }
        return tps;
    }

    private parseType(): TypeAnnot {
        // function type: `fn(T, U) -> R`
        if (this.eatKw("fn")) {
            this.expect(TokenType.LParen, "`(`");
            const params: TypeAnnot[] = [];
            if (!this.is(TokenType.RParen)) {
                do {
                    params.push(this.parseType());
                } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.RParen, "`)`");
            this.expect(TokenType.Arrow, "`->`");
            const ret = this.parseType();
            return { kind: "Fn", params, ret, span: this.spanHere() };
        }
        const start = this.spanHere();
        const name = this.expectIdent("type name");
        let ty: TypeAnnot;
        if (this.eat(TokenType.Lt)) {
            const args: TypeAnnot[] = [];
            if (!this.is(TokenType.Gt)) {
                do {
                    args.push(this.parseType());
                } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.Gt, "`>`");
            ty = { kind: "Generic", name, args, span: this.union(start, this.prevSpan()) };
        } else {
            ty = { kind: "Name", name, span: start };
        }
        // union types: `A | B`
        if (this.is(TokenType.Bar)) {
            const variants = [ty];
            while (this.eat(TokenType.Bar)) {
                const s = this.spanHere();
                const n = this.expectIdent("type name");
                let v: TypeAnnot;
                if (this.eat(TokenType.Lt)) {
                    const args: TypeAnnot[] = [];
                    if (!this.is(TokenType.Gt)) {
                        do { args.push(this.parseType()); } while (this.eat(TokenType.Comma));
                    }
                    this.expect(TokenType.Gt, "`>`");
                    v = { kind: "Generic", name: n, args, span: this.union(s, this.prevSpan()) };
                } else {
                    v = { kind: "Name", name: n, span: s };
                }
                variants.push(v);
            }
            return { kind: "Union", variants, span: this.union(start, this.prevSpan()) };
        }
        return ty;
    }

    // -----------------------------------------------------------------------
    // Expressions — Pratt parsing
    // -----------------------------------------------------------------------

    private parseExpr(minBp: number): Expr {
        let lhs = this.parsePrefix();

        for (;;) {
            // Assignment (right-assoc, only at min_bp 0).
            if (minBp === 0 && this.is(TokenType.Assign)) {
                const start = lhs.span;
                this.bump();
                const rhs = this.parseExpr(0);
                lhs = { kind: "Assign", target: lhs, value: rhs, span: this.union(start, rhs.span) };
                continue;
            }
            // Compound assignment.
            if (minBp === 0) {
                const cop = this.compoundOp();
                if (cop !== null) {
                    const start = lhs.span;
                    this.bump();
                    const rhs = this.parseExpr(0);
                    lhs = { kind: "CompoundAssign", op: cop, target: lhs, value: rhs, span: this.union(start, rhs.span) };
                    continue;
                }
            }
            const infix = this.infixOp();
            if (infix === null) break;
            const [lbp, rbp] = infix;
            if (lbp < minBp) break;
            const opTok = this.peek();
            this.bump();
            const rhs = this.parseExpr(rbp);
            lhs = this.buildInfix(opTok, lhs, rhs);
        }
        return lhs;
    }

    private compoundOp(): BinOp | null {
        switch (this.peek().type) {
            case TokenType.PlusAssign: return "Add";
            case TokenType.MinusAssign: return "Sub";
            case TokenType.StarAssign: return "Mul";
            case TokenType.SlashAssign: return "Div";
            case TokenType.PercentAssign: return "Mod";
            default: return null;
        }
    }

    /** Returns [leftBp, rightBp] for the current infix token, or null. */
    private infixOp(): [number, number] | null {
        const t = this.peek();
        if (t.type === TokenType.Pipe) return [1, 2];
        if (t.type === TokenType.Bang) return [3, 4];
        if (t.type === TokenType.Question) return [3, 4];
        if (t.type === TokenType.Or) return [5, 6];
        if (t.type === TokenType.Keyword && t.text === "or") return [5, 6];
        if (t.type === TokenType.And) return [7, 8];
        if (t.type === TokenType.Keyword && t.text === "and") return [7, 8];
        switch (t.type) {
            case TokenType.Eq:
            case TokenType.Neq:
            case TokenType.Lt:
            case TokenType.Gt:
            case TokenType.Lte:
            case TokenType.Gte:
                return [9, 10];
            case TokenType.Plus:
            case TokenType.Minus:
                return [11, 12];
            case TokenType.Star:
            case TokenType.Slash:
            case TokenType.Percent:
                return [13, 14];
            default:
                return null;
        }
    }

    private buildInfix(opTok: Token, lhs: Expr, rhs: Expr): Expr {
        const span = this.union(lhs.span, rhs.span);
        switch (opTok.type) {
            case TokenType.Pipe: return { kind: "Pipe", lhs, rhs, span };
            case TokenType.Bang: return { kind: "ActorSend", actor: lhs, msg: rhs, span };
            case TokenType.Question: return { kind: "ActorRequest", actor: lhs, msg: rhs, span };
            case TokenType.Or:
            case TokenType.And: {
                const op: BinOp = opTok.type === TokenType.And ? "And" : "Or";
                return { kind: "BinOp", op, lhs, rhs, span };
            }
            case TokenType.Eq: return { kind: "BinOp", op: "Eq", lhs, rhs, span };
            case TokenType.Neq: return { kind: "BinOp", op: "Neq", lhs, rhs, span };
            case TokenType.Lt: return { kind: "BinOp", op: "Lt", lhs, rhs, span };
            case TokenType.Gt: return { kind: "BinOp", op: "Gt", lhs, rhs, span };
            case TokenType.Lte: return { kind: "BinOp", op: "Lte", lhs, rhs, span };
            case TokenType.Gte: return { kind: "BinOp", op: "Gte", lhs, rhs, span };
            case TokenType.Plus: return { kind: "BinOp", op: "Add", lhs, rhs, span };
            case TokenType.Minus: return { kind: "BinOp", op: "Sub", lhs, rhs, span };
            case TokenType.Star: return { kind: "BinOp", op: "Mul", lhs, rhs, span };
            case TokenType.Slash: return { kind: "BinOp", op: "Div", lhs, rhs, span };
            case TokenType.Percent: return { kind: "BinOp", op: "Mod", lhs, rhs, span };
            default: {
                // `or`/`and` keywords
                if (opTok.type === TokenType.Keyword && opTok.text === "or")
                    return { kind: "BinOp", op: "Or", lhs, rhs, span };
                if (opTok.type === TokenType.Keyword && opTok.text === "and")
                    return { kind: "BinOp", op: "And", lhs, rhs, span };
                return { kind: "BinOp", op: "Add", lhs, rhs, span };
            }
        }
    }

    private parsePrefix(): Expr {
        const start = this.spanHere();
        if (this.is(TokenType.Minus)) {
            this.bump();
            const operand = this.parseExpr(15);
            return { kind: "UnaryOp", op: "Neg", expr: operand, span: this.union(start, operand.span) };
        }
        if (this.isKw("not")) {
            this.bump();
            const operand = this.parseExpr(15);
            return { kind: "UnaryOp", op: "Not", expr: operand, span: this.union(start, operand.span) };
        }
        return this.parsePostfix();
    }

    private parsePostfix(): Expr {
        let lhs = this.parsePrimary();
        for (;;) {
            const t = this.peek();
            if (t.type === TokenType.LParen) {
                this.bump();
                const args = this.parseArgs();
                lhs = { kind: "Call", callee: lhs, args, span: this.union(lhs.span, this.prevSpan()) };
            } else if (t.type === TokenType.LBracket) {
                this.bump();
                const index = this.parseExpr(0);
                this.expect(TokenType.RBracket, "`]`");
                lhs = { kind: "Index", target: lhs, index, span: this.union(lhs.span, this.prevSpan()) };
            } else if (t.type === TokenType.Dot) {
                this.bump();
                // Accept keywords as method/field names (e.g. parallel.spawn).
                const nameTok = this.peek();
                let name: string;
                if (nameTok.type === TokenType.Ident || nameTok.type === TokenType.Keyword) {
                    this.bump();
                    name = nameTok.text;
                } else {
                    this.error(this.spanHere(), `expected field or method name, found ${this.describe(nameTok)}`);
                    name = "<error>";
                }
                if (this.is(TokenType.LParen)) {
                    this.bump();
                    const args = this.parseArgs();
                    lhs = { kind: "MethodCall", receiver: lhs, method: name, args, span: this.union(lhs.span, this.prevSpan()) };
                } else {
                    lhs = { kind: "Field", target: lhs, name, span: this.union(lhs.span, this.prevSpan()) };
                }
            } else {
                break;
            }
        }
        return lhs;
    }

    private parseArgs(): Expr[] {
        const args: Expr[] = [];
        if (!this.is(TokenType.RParen)) {
            do {
                args.push(this.parseExpr(0));
            } while (this.eat(TokenType.Comma));
        }
        this.expect(TokenType.RParen, "`)`");
        return args;
    }

    private parsePrimary(): Expr {
        const start = this.spanHere();
        const t = this.peek();

        // Literals
        if (t.type === TokenType.Int) { this.bump(); return { kind: "Int", text: t.text, span: start }; }
        if (t.type === TokenType.Decimal) { this.bump(); return { kind: "Decimal", text: t.text, span: start }; }
        if (t.type === TokenType.String) {
            this.bump();
            // Naive interpolation: store the raw string text as a single
            // literal part. Sub-expression typing of `{expr}` is a future
            // enhancement; for hover/completion this is sufficient.
            const inner = stripStringQuotes(t.text);
            const lit: StrLit = { parts: [{ kind: "Literal", text: inner }] };
            return { kind: "Str", lit, span: start };
        }
        if (this.isKw("true")) { this.bump(); return { kind: "Bool", value: true, span: start }; }
        if (this.isKw("false")) { this.bump(); return { kind: "Bool", value: false, span: start }; }
        if (this.isKw("nil")) { this.bump(); return { kind: "Nil", span: start }; }

        // Identifier
        if (t.type === TokenType.Ident) { this.bump(); return { kind: "Ident", name: t.text, span: start }; }

        // Vec literal `[a, b, c]`
        if (t.type === TokenType.LBracket) {
            this.bump();
            const items: Expr[] = [];
            if (!this.is(TokenType.RBracket)) {
                do { items.push(this.parseExpr(0)); } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.RBracket, "`]`");
            return { kind: "VecLit", items, span: this.union(start, this.prevSpan()) };
        }

        // Set literal `#{1, 2, 3}`
        if (t.type === TokenType.HashBrace) {
            this.bump();
            const items: Expr[] = [];
            if (!this.is(TokenType.RBrace)) {
                do { items.push(this.parseExpr(0)); } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.RBrace, "`}`");
            return { kind: "SetLit", items, span: this.union(start, this.prevSpan()) };
        }

        // Explicit map literal `\{ k: v, ... \}` — unambiguous form used in
        // positions where `{` would be read as a block (e.g. `return \{...\}`).
        if (t.type === TokenType.EscLBrace) {
            this.bump();
            const entries: Array<{ key: Expr; value: Expr }> = [];
            if (!this.is(TokenType.EscRBrace)) {
                do {
                    const k = this.parseExpr(0);
                    this.expect(TokenType.Colon, "`:`");
                    const v = this.parseExpr(0);
                    entries.push({ key: k, value: v });
                } while (this.eat(TokenType.Comma));
            }
            this.expect(TokenType.EscRBrace, "`\\}`");
            return { kind: "MapLit", entries, span: this.union(start, this.prevSpan()) };
        }

        // Parenthesized expression
        if (t.type === TokenType.LParen) {
            this.bump();
            const expr = this.parseExpr(0);
            this.expect(TokenType.RParen, "`)`");
            return { kind: "Paren", expr, span: this.union(start, this.prevSpan()) };
        }

        // Brace: block or map literal (decided by lookahead)
        if (t.type === TokenType.LBrace) {
            return this.parseBraceExpr(start);
        }

        // Lambda `fn(params) -> T { body }` (anonymous)
        if (this.isKw("fn") && this.peek(1).type === TokenType.LParen) {
            this.bump(); // `fn`
            const params = this.parseParams();
            const returnType = this.eat(TokenType.Arrow) ? this.parseType() : null;
            const body = this.parseExpr(0);
            return { kind: "Lambda", params, returnType, body, span: this.union(start, this.prevSpan()) };
        }

        // Control-flow keywords that start expressions
        if (this.isKw("if")) return this.parseIf(start);
        if (this.isKw("match")) return this.parseMatch(start);
        if (this.isKw("while")) {
            this.bump();
            const cond = this.parseExpr(0);
            const body = this.parseExpr(0);
            return { kind: "While", cond, body, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("for")) {
            this.bump();
            const v = this.expectIdent("loop variable");
            if (!this.eatKw("in")) this.error(this.spanHere(), "expected `in`");
            const iter = this.parseExpr(0);
            const body = this.parseExpr(0);
            return { kind: "For", var: v, iter, body, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("loop")) {
            this.bump();
            const body = this.parseExpr(0);
            return { kind: "Loop", body, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("break")) {
            this.bump();
            // optional break value: only consume an expr if the next token can
            // start one (avoids eating `;` or `}`).
            let value: Expr | null = null;
            if (canStartExpr(this.peek())) value = this.parseExpr(0);
            return { kind: "Break", value, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("continue")) {
            this.bump();
            return { kind: "Continue", span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("return")) {
            this.bump();
            let value: Expr | null = null;
            if (canStartExpr(this.peek())) value = this.parseExpr(0);
            return { kind: "Return", value, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("reply")) {
            this.bump();
            const value = this.parseExpr(0);
            return { kind: "Reply", value, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("yield")) {
            this.bump();
            return { kind: "Yield", span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("await")) {
            this.bump();
            const expr = this.parseExpr(15);
            return { kind: "Await", expr, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("raise")) {
            this.bump();
            const expr: Expr = canStartExpr(this.peek())
                ? this.parseExpr(0)
                : { kind: "Nil", span: this.spanHere() };
            return { kind: "Raise", expr, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("try")) return this.parseTry(start);
        if (this.isKw("transact")) {
            this.bump();
            const body = this.parseExpr(0);
            return { kind: "Transact", body, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("retry")) {
            this.bump();
            return { kind: "Retry", span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("shared")) {
            // `shared expr` — expression form (creates a Shared cell).
            this.bump();
            const expr = this.parseExpr(15);
            return { kind: "SharedExpr", expr, span: this.union(start, this.prevSpan()) };
        }
        if (this.isKw("spawn")) {
            this.bump();
            const name = this.expectIdent("actor name after `spawn`");
            this.expect(TokenType.LParen, "`(`");
            const args = this.parseArgs();
            return { kind: "Spawn", name, args, span: this.union(start, this.prevSpan()) };
        }

        this.error(start, `unexpected ${this.describe(t)} in expression`);
        // Consume to avoid an infinite loop; return a nil placeholder.
        this.bump();
        return { kind: "Nil", span: start };
    }

    private parseBraceExpr(start: Span): Expr {
        this.bump(); // `{`
        // Empty `{}` is an empty map literal.
        if (this.is(TokenType.RBrace)) {
            this.bump();
            return { kind: "MapLit", entries: [], span: this.union(start, this.prevSpan()) };
        }
        // Declaration keyword ⇒ block.
        const t1 = this.peek();
        if (t1.type === TokenType.Keyword && DECL_KEYWORDS.has(t1.text)) {
            return this.finishBlock(start);
        }
        // Parse one expression; if `:` follows, it's a map.
        const first = this.parseExpr(0);
        if (this.is(TokenType.Colon)) {
            this.bump();
            const v = this.parseExpr(0);
            const entries: Array<{ key: Expr; value: Expr }> = [{ key: first, value: v }];
            while (this.eat(TokenType.Comma)) {
                if (this.is(TokenType.RBrace)) break; // trailing comma
                const k = this.parseExpr(0);
                this.expect(TokenType.Colon, "`:`");
                const val = this.parseExpr(0);
                entries.push({ key: k, value: val });
            }
            this.expect(TokenType.RBrace, "`}`");
            return { kind: "MapLit", entries, span: this.union(start, this.prevSpan()) };
        }
        // Block: `first` is a Semi stmt or the tail.
        const stmts: Stmt[] = [];
        let tail: Expr | null = null;
        if (this.eat(TokenType.Semicolon)) {
            stmts.push({ kind: "Semi", expr: first, span: first.span });
            tail = this.continueBlockStmts(stmts);
        } else if (this.is(TokenType.RBrace)) {
            tail = first;
        } else {
            this.error(this.spanHere(), `expected \`;\` or \`}\` in block, found ${this.describe(this.peek())}`);
            tail = first;
        }
        this.expect(TokenType.RBrace, "`}`");
        return { kind: "Block", stmts, tail, span: this.union(start, this.prevSpan()) };
    }

    private finishBlock(start: Span): Expr {
        const stmts: Stmt[] = [];
        const tail = this.continueBlockStmts(stmts);
        this.expect(TokenType.RBrace, "`}`");
        return { kind: "Block", stmts, tail, span: this.union(start, this.prevSpan()) };
    }

    /** Parse the remainder of a block body (after the first stmt). Returns tail. */
    private continueBlockStmts(stmts: Stmt[]): Expr | null {
        while (!this.is(TokenType.RBrace) && !this.is(TokenType.Eof)) {
            const s = this.parseStmt();
            if (!s) continue;
            if (s.kind === "Expr") {
                // A bare expression (no trailing `;`) is the block's tail.
                return s.expr;
            }
            // Declarations and `;`-terminated exprs: consume an optional
            // trailing `;` (decls like `let` don't consume it themselves).
            if (this.is(TokenType.Semicolon)) this.bump();
            stmts.push(s);
        }
        return null;
    }

    private parseIf(start: Span): Expr {
        this.bump(); // `if`
        const cond = this.parseExpr(0);
        const then = this.parseExpr(0);
        let else_: Expr | null = null;
        if (this.eatKw("else")) {
            // `else if` chains by recursing into parseExpr → parseIf.
            else_ = this.parseExpr(0);
        }
        return { kind: "If", cond, then, else_, span: this.union(start, this.prevSpan()) };
    }

    private parseMatch(start: Span): Expr {
        this.bump(); // `match`
        const scrutinee = this.parseExpr(0);
        this.expect(TokenType.LBrace, "`{`");
        const arms: MatchArm[] = [];
        while (!this.is(TokenType.RBrace) && !this.is(TokenType.Eof)) {
            const astart = this.spanHere();
            const pattern = this.parsePattern();
            let guard: Expr | null = null;
            if (this.eatKw("if")) guard = this.parseExpr(0);
            this.expect(TokenType.FatArrow, "`=>`");
            const body = this.parseExpr(0);
            arms.push({ pattern, guard, body, span: this.union(astart, this.prevSpan()) });
            if (!this.eat(TokenType.Comma)) {
                // Arms may also be separated by nothing if bodies are blocks.
                if (this.is(TokenType.RBrace)) break;
            }
        }
        this.expect(TokenType.RBrace, "`}`");
        return { kind: "Match", scrutinee, arms, span: this.union(start, this.prevSpan()) };
    }

    private parsePattern(): Pattern {
        const start = this.spanHere();
        // or-pattern: parse first arm, then `|`-separated alternatives.
        const first = this.parsePatternAtom();
        if (!this.is(TokenType.Bar)) return first;
        const pats = [first];
        while (this.eat(TokenType.Bar)) {
            pats.push(this.parsePatternAtom());
        }
        return { kind: "Or", pats, span: this.union(start, this.prevSpan()) };
    }

    private parsePatternAtom(): Pattern {
        const start = this.spanHere();
        const t = this.peek();
        // `_` is tokenized as an Ident with text "_".
        if (t.type === TokenType.Ident) {
            if (t.text === "_") {
                this.bump();
                return { kind: "Wildcard", span: start };
            }
            const isUpper = /^[A-Z]/.test(t.text);
            this.bump();
            // Named struct pattern: `Name { field: pat, ... }` (uppercase + `{`).
            if (isUpper && this.is(TokenType.LBrace)) {
                this.bump(); // `{`
                const fields: Array<{ name: string; pat: Pattern }> = [];
                let rest = false;
                if (!this.is(TokenType.RBrace)) {
                    do {
                        if (this.is(TokenType.DotDot)) { this.bump(); rest = true; break; }
                        const fname = this.expectIdent("field name in struct pattern");
                        let pat: Pattern;
                        if (this.eat(TokenType.Colon)) {
                            pat = this.parsePattern();
                        } else {
                            // shorthand `{ x }` ⇒ binds `x`
                            pat = { kind: "Bind", name: fname, span: this.spanHere() };
                        }
                        fields.push({ name: fname, pat });
                    } while (this.eat(TokenType.Comma));
                }
                if (this.is(TokenType.DotDot)) { this.bump(); rest = true; }
                this.expect(TokenType.RBrace, "`}`");
                return { kind: "Struct", name: t.text, fields, rest, span: this.union(start, this.prevSpan()) };
            }
            // Variant pattern: `Name(args)` (uppercase convention) or `Name`.
            if (this.is(TokenType.LParen)) {
                this.bump();
                const args: Pattern[] = [];
                if (!this.is(TokenType.RParen)) {
                    do { args.push(this.parsePattern()); } while (this.eat(TokenType.Comma));
                }
                this.expect(TokenType.RParen, "`)`");
                return { kind: "Variant", name: t.text, args, span: this.union(start, this.prevSpan()) };
            }
            // Uppercase without args ⇒ variant with no args (e.g. `None`).
            if (isUpper) return { kind: "Variant", name: t.text, args: [], span: start };
            return { kind: "Bind", name: t.text, span: start };
        }
        // Literal patterns
        if (t.type === TokenType.Int) { this.bump(); return { kind: "Lit", lit: { kind: "Int", text: t.text }, span: start }; }
        if (t.type === TokenType.Decimal) { this.bump(); return { kind: "Lit", lit: { kind: "Decimal", text: t.text }, span: start }; }
        if (t.type === TokenType.String) {
            this.bump();
            const inner = stripStringQuotes(t.text);
            return { kind: "Lit", lit: { kind: "Str", parts: [{ kind: "Literal", text: inner }] }, span: start };
        }
        if (this.isKw("true")) { this.bump(); return { kind: "Lit", lit: { kind: "Bool", value: true }, span: start }; }
        if (this.isKw("false")) { this.bump(); return { kind: "Lit", lit: { kind: "Bool", value: false }, span: start }; }
        if (this.isKw("nil")) { this.bump(); return { kind: "Lit", lit: { kind: "Nil" }, span: start }; }
        // Negative int pattern: `-1`
        if (t.type === TokenType.Minus && this.peek(1).type === TokenType.Int) {
            this.bump();
            const nt = this.bump();
            return { kind: "Lit", lit: { kind: "Int", text: "-" + nt.text }, span: this.union(start, this.prevSpan()) };
        }
        // Struct pattern `{ field: pat, .. }`
        if (t.type === TokenType.LBrace) {
            this.bump();
            const fields: Array<{ name: string; pat: Pattern }> = [];
            let rest = false;
            if (!this.is(TokenType.RBrace)) {
                do {
                    if (this.is(TokenType.DotDot)) { this.bump(); rest = true; break; }
                    const fname = this.expectIdent("field name");
                    let pat: Pattern;
                    if (this.eat(TokenType.Colon)) {
                        pat = this.parsePattern();
                    } else {
                        // shorthand: bind to field name
                        pat = { kind: "Bind", name: fname, span: this.spanHere() };
                    }
                    fields.push({ name: fname, pat });
                } while (this.eat(TokenType.Comma));
            }
            if (this.is(TokenType.DotDot)) { this.bump(); rest = true; }
            this.expect(TokenType.RBrace, "`}`");
            return { kind: "Struct", name: null, fields, rest, span: this.union(start, this.prevSpan()) };
        }
        // Vec pattern `[a, b, ..rest]`
        if (t.type === TokenType.LBracket) {
            this.bump();
            const pats: Pattern[] = [];
            let rest = false;
            if (!this.is(TokenType.RBracket)) {
                do {
                    if (this.is(TokenType.DotDot)) { this.bump(); rest = true; break; }
                    pats.push(this.parsePattern());
                } while (this.eat(TokenType.Comma));
            }
            if (this.is(TokenType.DotDot)) { this.bump(); rest = true; }
            this.expect(TokenType.RBracket, "`]`");
            return { kind: "Vec", pats, rest, span: this.union(start, this.prevSpan()) };
        }
        this.error(start, `unexpected ${this.describe(t)} in pattern`);
        this.bump();
        return { kind: "Wildcard", span: start };
    }

    private parseTry(start: Span): Expr {
        this.bump(); // `try`
        const body = this.parseExpr(0);
        const rescues: RescueClause[] = [];
        while (this.eatKw("rescue")) {
            const rstart = this.spanHere();
            let typeName: string | null = null;
            let bind: string | null = null;
            // `rescue TypeName as e` | `rescue as e` | `rescue TypeName` | `rescue`
            const t = this.peek();
            if (t.type === TokenType.Ident || (t.type === TokenType.Keyword && !isReservedKeyword(t.text))) {
                typeName = t.text;
                this.bump();
            }
            if (this.eatKw("as")) bind = this.expectIdent("binding name");
            const rbody = this.parseExpr(0);
            rescues.push({ typeName, bind, body: rbody, span: this.union(rstart, this.prevSpan()) });
        }
        let ensure: Expr | null = null;
        if (this.eatKw("ensure")) ensure = this.parseExpr(0);
        return { kind: "Try", body, rescues, ensure, span: this.union(start, this.prevSpan()) };
    }

    // -----------------------------------------------------------------------
    // Recovery
    // -----------------------------------------------------------------------

    private synchronize(): void {
        // Skip tokens until we reach a likely statement boundary.
        while (!this.is(TokenType.Eof)) {
            const t = this.peek();
            if (t.type === TokenType.Semicolon || t.type === TokenType.RBrace) { this.bump(); return; }
            if (t.type === TokenType.Keyword && DECL_KEYWORDS.has(t.text)) return;
            this.bump();
        }
    }

    private firstStart(stmts: Stmt[]): number {
        return stmts.length ? stmts[0].span.start : 0;
    }
    private lastEnd(stmts: Stmt[]): number {
        return stmts.length ? stmts[stmts.length - 1].span.end : 0;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Keywords that are reserved and cannot be used as identifiers. */
function isReservedKeyword(text: string): boolean {
    // Only the structural keywords are reserved; type/literal keywords like
    // `true`/`nil` could appear as field names but we keep it conservative.
    const reserved = new Set([
        "let", "shared", "state", "fn", "type", "enum", "actor", "on",
        "import", "lazy", "if", "else", "match", "while", "for", "in",
        "loop", "break", "continue", "return", "reply", "yield", "await",
        "raise", "try", "rescue", "ensure", "transact", "retry", "spawn",
        "true", "false", "nil", "and", "or", "not", "as",
    ]);
    return reserved.has(text);
}

function canStartExpr(t: Token): boolean {
    switch (t.type) {
        case TokenType.Int:
        case TokenType.Decimal:
        case TokenType.String:
        case TokenType.Ident:
        case TokenType.LParen:
        case TokenType.LBrace:
        case TokenType.LBracket:
        case TokenType.HashBrace:
        case TokenType.EscLBrace:
        case TokenType.Minus:
            return true;
        case TokenType.Keyword:
            return [
                "true", "false", "nil", "if", "match", "while", "for", "loop",
                "break", "continue", "return", "reply", "yield", "await",
                "raise", "try", "transact", "retry", "shared", "spawn", "fn",
                "not",
            ].includes(t.text);
        default:
            return false;
    }
}

/** Strip the surrounding quotes from a string token's text. */
function stripStringQuotes(text: string): string {
    if (text.startsWith('"""') && text.endsWith('"""') && text.length >= 6) {
        return text.slice(3, -3);
    }
    if (text.startsWith('"') && text.endsWith('"') && text.length >= 2) {
        return text.slice(1, -1);
    }
    return text;
}
