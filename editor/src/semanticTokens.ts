// Semantic token classification for the 1y language server.
//
// Drives `textDocument/semanticTokens/full`: assigns a token type + modifier
// set to each identifier/keyword/literal/comment in the source so the editor
// can color functions, types, variables, parameters, modules, etc. distinctly
// (on top of the TextMate grammar, which only does regex-based coloring).
//
// Classification resolves each identifier through the AST symbol table, so
// `count` colors as a builtin function, a user `count` shadows it, and
// `Vec<Int>` colors `Vec` as a type. Declaration name spans get a
// `declaration` modifier; documented declarations get `documentation`.

import { Token, TokenType } from "./lexer";
import { SymbolTable, SymbolInfo, resolveAt } from "./symbols";
import { BUILTIN_COMPLETIONS, MODULE_COMPLETIONS } from "./completions";

// ---------------------------------------------------------------------------
// Legend — token types and modifiers, in the order the LSP expects indices.
// ---------------------------------------------------------------------------

export const SEMANTIC_TOKEN_TYPES = [
    "type",         // 0  — type definitions (`type Point`) and type references
    "class",        // 1  — actor definitions
    "enum",         // 2  — enum definitions
    "enumMember",   // 3  — enum variants
    "function",     // 4  — user functions, variant constructors, builtins
    "method",       // 5  — `on` handlers
    "variable",     // 6  — let/shared/state/match_bind/for_var
    "parameter",    // 7  — function/lambda params
    "module",       // 8  — import aliases (`json` in `import json as J`)
    "keyword",      // 9  — reserved keywords
    "comment",      // 10 — `//` comments
    "string",       // 11 — string literals
    "number",       // 12 — Int / Decimal literals
    "macro",        // 13 — module functions (`stringify` in `json.stringify`)
    "operator",     // 14 — reserved for future operator coloring
] as const;

export const SEMANTIC_TOKEN_MODIFIERS = [
    "declaration",     // 0 — the declaration's own name span
    "documentation",   // 1 — has a `///` doc comment
    "readonly",        // 2 — reserved (e.g. nil/true/false literals)
    "defaultLibrary",  // 3 — builtin / stdlib symbol
] as const;

export interface SemanticTokenLegend {
    tokenTypes: readonly string[];
    tokenModifiers: readonly string[];
}

export const SEMANTIC_TOKEN_LEGEND: SemanticTokenLegend = {
    tokenTypes: SEMANTIC_TOKEN_TYPES,
    tokenModifiers: SEMANTIC_TOKEN_MODIFIERS,
};

// ---------------------------------------------------------------------------
// Token classification
// ---------------------------------------------------------------------------

/** A single classified token: absolute position + type/modifier indices. */
interface ClassifiedToken {
    line: number;
    col: number;
    length: number;
    typeIdx: number;
    modifierBitmask: number;
}

const MOD_DECLARATION = 1 << 0;
const MOD_DOCUMENTATION = 1 << 1;
const MOD_READONLY = 1 << 2;
const MOD_DEFAULT_LIBRARY = 1 << 3;

/** Builtins lookup set (count, map, println, ...). */
const BUILTIN_NAMES: ReadonlySet<string> = new Set(
    BUILTIN_COMPLETIONS.map((b) => b.label),
);

/** Module names lookup set (json, io, env, ...). */
const MODULE_NAMES: ReadonlySet<string> = new Set(
    Object.keys(MODULE_COMPLETIONS),
);

/**
 * Classify all tokens in `tokens` and return the LSP-encoded data array
 * (5-tuples of deltaLine, deltaStart, length, tokenType, tokenModifiers).
 */
export function classifySemanticTokens(
    tokens: Token[],
    table: SymbolTable,
): number[] {
    const classified: ClassifiedToken[] = [];

    for (let i = 0; i < tokens.length; i++) {
        const tok = tokens[i];

        switch (tok.type) {
            case TokenType.Comment:
                push(classified, tok, 10 /* comment */, 0);
                continue;
            case TokenType.DocComment:
                push(classified, tok, 10 /* comment */, MOD_DOCUMENTATION);
                continue;
            case TokenType.Int:
            case TokenType.Decimal:
                push(classified, tok, 12 /* number */, 0);
                continue;
            case TokenType.String:
                push(classified, tok, 11 /* string */, 0);
                continue;
            case TokenType.Keyword: {
                // `true` / `false` / `nil` are readonly literal keywords.
                if (tok.text === "true" || tok.text === "false" || tok.text === "nil") {
                    push(classified, tok, 9 /* keyword */, MOD_READONLY);
                } else {
                    push(classified, tok, 9 /* keyword */, 0);
                }
                continue;
            }
            case TokenType.Ident: {
                const c = classifyIdent(tok, i, tokens, table);
                if (c) push(classified, tok, c.typeIdx, c.modifiers);
                continue;
            }
        }
    }

    return encode(classified);
}

interface Class {
    typeIdx: number;
    modifiers: number;
}

/** Classify an identifier token using the symbol table + surrounding context. */
function classifyIdent(
    tok: Token,
    idx: number,
    tokens: Token[],
    table: SymbolTable,
): Class | null {
    const name = tok.text;

    // 1. Resolve through the symbol table (scope-aware).
    const sym = resolveAt(table, tok.start, name);

    // 2. Declaration-name span — add `declaration` modifier.
    const isDecl = sym != null
        && tok.start >= sym.nameSpan.start
        && tok.start < sym.nameSpan.end;

    // 3. Type position: token preceded by `:` or `->`, or inside `<...>`.
    //    Color as a type. (Best-effort: only the simple `Name` case.)
    if (isInTypePosition(tokens, idx)) {
        return { typeIdx: 0 /* type */, modifiers: modBits(isDecl, sym, false) };
    }

    // 4. Field access `prefix.name` — if prefix is a module/import, color
    //    the member as a macro (stdlib function).
    const prevNonTrivial = prevSignificant(tokens, idx);
    if (prevNonTrivial && prevNonTrivial.type === TokenType.Dot) {
        const prefixTok = prevSignificant(tokens, prevNonTrivialIdx(tokens, idx));
        if (prefixTok) {
            const prefixSym = resolveAt(table, prefixTok.start, prefixTok.text);
            const isModule =
                (prefixSym != null && prefixSym.kind === "import")
                || MODULE_NAMES.has(prefixTok.text);
            if (isModule) {
                return { typeIdx: 13 /* macro */, modifiers: MOD_DEFAULT_LIBRARY };
            }
        }
        // Field access on a non-module — leave uncolored (fall through).
    }

    // 5. Resolved user symbol — classify by kind.
    if (sym) {
        return { typeIdx: typeIdxForKind(sym.kind), modifiers: modBits(isDecl, sym, false) };
    }

    // 6. Unresolved — check builtins and module names.
    if (BUILTIN_NAMES.has(name)) {
        return { typeIdx: 4 /* function */, modifiers: MOD_DEFAULT_LIBRARY };
    }
    if (MODULE_NAMES.has(name)) {
        return { typeIdx: 8 /* module */, modifiers: MOD_DEFAULT_LIBRARY };
    }

    // 7. Followed by `(` and unresolved — guess function.
    const nextSig = nextSignificant(tokens, idx);
    if (nextSig && nextSig.type === TokenType.LParen) {
        return { typeIdx: 4 /* function */, modifiers: 0 };
    }

    // 8. Default: plain variable.
    return { typeIdx: 6 /* variable */, modifiers: 0 };
}

/** Map a 1y SymbolKind to a semantic token type index. */
function typeIdxForKind(kind: SymbolInfo["kind"]): number {
    switch (kind) {
        case "function": return 4;       // function
        case "on": return 5;             // method
        case "let": case "shared": case "state":
        case "for_var": case "match_bind": return 6; // variable
        case "param": case "lambda_param": return 7; // parameter
        case "type": return 0;           // type
        case "enum": return 2;           // enum
        case "variant": return 3;        // enumMember
        case "actor": return 1;          // class
        case "import": return 8;         // module
    }
}

/** Build the modifier bitmask from declaration-ness + doc presence. */
function modBits(isDecl: boolean, sym: SymbolInfo | null, isDefaultLib: boolean): number {
    let m = 0;
    if (isDecl) m |= MOD_DECLARATION;
    if (sym && sym.doc) m |= MOD_DOCUMENTATION;
    if (isDefaultLib) m |= MOD_DEFAULT_LIBRARY;
    return m;
}

// ---------------------------------------------------------------------------
// Type-position + neighbor-token helpers
// ---------------------------------------------------------------------------

/** True if the token at `idx` is in a type-annotation position. */
function isInTypePosition(tokens: Token[], idx: number): boolean {
    const prev = prevSignificant(tokens, idx);
    if (!prev) return false;
    if (prev.type === TokenType.Colon) return true;
    if (prev.type === TokenType.Arrow) return true;
    // Inside `<...>` of a generic like `Vec<Int>`: previous non-trivial is
    // either `<`-equivalent (we use Lt for `<`) or a comma, and the
    // enclosing opener is `<`.
    if (prev.type === TokenType.Comma || prev.type === TokenType.Lt) {
        return isInsideAngleBrackets(tokens, idx);
    }
    return false;
}

/** True if `idx` sits inside a `<...>` angle-bracket group. */
function isInsideAngleBrackets(tokens: Token[], idx: number): boolean {
    let depth = 0;
    for (let j = idx - 1; j >= 0; j--) {
        const t = tokens[j];
        if (t.type === TokenType.Gt) depth++;
        else if (t.type === TokenType.Lt) {
            if (depth === 0) return true;
            depth--;
        }
    }
    return false;
}

function prevSignificant(tokens: Token[], idx: number): Token | null {
    for (let j = idx - 1; j >= 0; j--) {
        const t = tokens[j];
        // Whitespace tokens don't exist in our lexer (they're skipped), so
        // every token is "significant". Keep the helper for clarity.
        return t;
    }
    return null;
}

function prevNonTrivialIdx(tokens: Token[], idx: number): number {
    return Math.max(0, idx - 1);
}

function nextSignificant(tokens: Token[], idx: number): Token | null {
    return idx + 1 < tokens.length ? tokens[idx + 1] : null;
}

// ---------------------------------------------------------------------------
// Encoding — produce the LSP delta-encoded flat array
// ---------------------------------------------------------------------------

function push(
    out: ClassifiedToken[],
    tok: Token,
    typeIdx: number,
    modifiers: number,
): void {
    out.push({
        // LSP semantic tokens use 0-based line/col, but the lexer tracks
        // 1-based. Convert here so the delta encoding is correct.
        line: tok.line - 1,
        col: tok.col - 1,
        length: tok.end - tok.start,
        typeIdx,
        modifierBitmask: modifiers,
    });
}

/** Delta-encode classified tokens into the LSP data array. */
function encode(tokens: ClassifiedToken[]): number[] {
    // Sort by (line, col) just in case.
    tokens.sort((a, b) => a.line - b.line || a.col - b.col);
    const data: number[] = [];
    let prevLine = 0;
    let prevCol = 0;
    for (const t of tokens) {
        if (t.line !== prevLine) {
            prevCol = 0;
        }
        data.push(
            t.line - prevLine,
            t.col - prevCol,
            t.length,
            t.typeIdx,
            t.modifierBitmask,
        );
        prevLine = t.line;
        prevCol = t.col;
    }
    return data;
}
