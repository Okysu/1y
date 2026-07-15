// Lightweight 1y lexer in TypeScript for basic syntax checking and tokenization.
// This is NOT a full reimplementation of the Rust lexer — it covers the common
// cases needed for diagnostics and completions in the editor.

export enum TokenType {
    Ident,
    Keyword,
    Int,
    Decimal,
    String,
    StringStart,    // opening " of an unterminated string
    Comment,
    LParen, RParen,
    LBrace, RBrace,
    LBracket, RBracket,
    Comma, Semicolon, Colon, DoubleColon,
    Dot, DotDot,
    Arrow, FatArrow, Pipe,
    Assign, PlusAssign, MinusAssign, StarAssign, SlashAssign, PercentAssign,
    Eq, Neq, Lt, Gt, Lte, Gte,
    Plus, Minus, Star, Slash, Percent,
    Bang, And, Or,
    Question,
    HashBrace,
    Bar,            // single `|` — Or-pattern separator
    EscLBrace,      // `\{` — escaped opening brace (Map literal)
    EscRBrace,      // `\}` — escaped closing brace (Map literal)
    Eof,
    Unknown,
}

export interface Token {
    type: TokenType;
    text: string;
    line: number;
    col: number;
    start: number;  // offset in source
    end: number;
}

export const KEYWORDS: ReadonlySet<string> = new Set([
    "let", "fn", "if", "else", "match", "while", "for", "in", "break",
    "continue", "loop", "enum", "type", "struct", "actor", "state", "on",
    "spawn", "reply", "receive", "shared", "transact", "retry", "import",
    "lazy", "as", "raise", "try", "rescue", "ensure", "return", "true",
    "false", "nil", "and", "or", "not",
]);

export function tokenize(src: string): Token[] {
    const tokens: Token[] = [];
    let i = 0;
    let line = 1;
    let col = 1;
    const n = src.length;

    const advance = (count = 1) => {
        for (let k = 0; k < count; k++) {
            if (src[i] === "\n") { line++; col = 1; } else { col++; }
            i++;
        }
    };

    const peek = (offset = 0): string => (i + offset < n) ? src[i + offset] : "";

    const push = (type: TokenType, text: string, len: number) => {
        tokens.push({ type, text, line, col, start: i, end: i + len });
        advance(len);
    };

    while (i < n) {
        const startLine = line;
        const startCol = col;
        const c = src[i];

        // Whitespace
        if (c === " " || c === "\t" || c === "\r" || c === "\n") {
            advance();
            continue;
        }

        // Line comment
        if (c === "/" && peek(1) === "/") {
            let len = 2;
            while (i + len < n && src[i + len] !== "\n") len++;
            tokens.push({ type: TokenType.Comment, text: src.substr(i, len), line: startLine, col: startCol, start: i, end: i + len });
            advance(len);
            continue;
        }

        // Block comment (nestable)
        if (c === "/" && peek(1) === "*") {
            let depth = 1;
            let len = 2;
            while (i + len < n && depth > 0) {
                if (src[i + len] === "/" && src[i + len + 1] === "*") { depth++; len += 2; }
                else if (src[i + len] === "*" && src[i + len + 1] === "/") { depth--; len += 2; }
                else { len++; }
            }
            tokens.push({ type: TokenType.Comment, text: src.substr(i, len), line: startLine, col: startCol, start: i, end: i + len });
            advance(len);
            continue;
        }

        // Identifiers / keywords
        if (isIdentStart(c)) {
            let len = 1;
            while (i + len < n && isIdentContinue(src[i + len])) len++;
            const text = src.substr(i, len);
            const type = KEYWORDS.has(text) ? TokenType.Keyword : TokenType.Ident;
            tokens.push({ type, text, line: startLine, col: startCol, start: i, end: i + len });
            advance(len);
            continue;
        }

        // Numbers
        if (c >= "0" && c <= "9") {
            let len = 1;
            while (i + len < n && (isDigit(src[i + len]) || src[i + len] === "_")) len++;
            let type = TokenType.Int;
            if (src[i + len] === "." && isDigit(src[i + len + 1])) {
                len++;
                while (i + len < n && (isDigit(src[i + len]) || src[i + len] === "_")) len++;
                type = TokenType.Decimal;
                // optional exponent
                if (src[i + len] === "e" || src[i + len] === "E") {
                    len++;
                    if (src[i + len] === "+" || src[i + len] === "-") len++;
                    while (i + len < n && isDigit(src[i + len])) len++;
                }
            }
            tokens.push({ type, text: src.substr(i, len), line: startLine, col: startCol, start: i, end: i + len });
            advance(len);
            continue;
        }

        // Strings (with interpolation — we treat interpolation naively as part of string)
        if (c === '"') {
            let len = 1;
            let terminated = false;
            // Triple-quoted?
            if (peek(1) === '"' && peek(2) === '"') {
                len = 3;
                while (i + len < n) {
                    if (src[i + len] === '"' && src[i + len + 1] === '"' && src[i + len + 2] === '"') {
                        len += 3;
                        terminated = true;
                        break;
                    }
                    len++;
                }
            } else {
                while (i + len < n) {
                    if (src[i + len] === "\\") { len += 2; continue; }
                    if (src[i + len] === '"') { len++; terminated = true; break; }
                    len++;
                }
            }
            tokens.push({
                type: terminated ? TokenType.String : TokenType.StringStart,
                text: src.substr(i, len),
                line: startLine, col: startCol, start: i, end: i + len,
            });
            advance(len);
            continue;
        }

        // Escaped braces: `\{` and `\}` — Map literal delimiters in 1y.
        // Must be checked before the generic punctuation pass so the backslash
        // is not reported as an unknown character.
        if (c === "\\" && (peek(1) === "{" || peek(1) === "}")) {
            const text = src.substr(i, 2);
            const type = peek(1) === "{" ? TokenType.EscLBrace : TokenType.EscRBrace;
            tokens.push({ type, text, line: startLine, col: startCol, start: i, end: i + 2 });
            advance(2);
            continue;
        }

        // Punctuation & operators
        const two = src.substr(i, 2);
        const three = src.substr(i, 3);
        const singles: Record<string, TokenType> = {
            "(": TokenType.LParen, ")": TokenType.RParen,
            "{": TokenType.LBrace, "}": TokenType.RBrace,
            "[": TokenType.LBracket, "]": TokenType.RBracket,
            ",": TokenType.Comma, ";": TokenType.Semicolon,
            ":": TokenType.Colon, ".": TokenType.Dot,
            "+": TokenType.Plus, "-": TokenType.Minus,
            "*": TokenType.Star, "/": TokenType.Slash, "%": TokenType.Percent,
            "<": TokenType.Lt, ">": TokenType.Gt, "!": TokenType.Bang,
            "?": TokenType.Question, "=": TokenType.Assign,
            "|": TokenType.Bar,
        };
        const doubles: Record<string, TokenType> = {
            "->": TokenType.Arrow, "=>": TokenType.FatArrow,
            "|>": TokenType.Pipe, "==": TokenType.Eq, "!=": TokenType.Neq,
            "<=": TokenType.Lte, ">=": TokenType.Gte,
            "+=": TokenType.PlusAssign, "-=": TokenType.MinusAssign,
            "*=": TokenType.StarAssign, "/=": TokenType.SlashAssign,
            "%=": TokenType.PercentAssign, "&&": TokenType.And, "||": TokenType.Or,
            "::": TokenType.DoubleColon, "..": TokenType.DotDot,
            "#{": TokenType.HashBrace,
        };
        if (doubles[two]) { push(doubles[two], two, 2); continue; }
        if (singles[c]) { push(singles[c], c, 1); continue; }

        // Unknown
        push(TokenType.Unknown, c, 1);
    }

    tokens.push({ type: TokenType.Eof, text: "", line, col, start: i, end: i });
    return tokens;
}

function isIdentStart(c: string): boolean {
    return (c >= "a" && c <= "z") || (c >= "A" && c <= "Z") || c === "_";
}

function isIdentContinue(c: string): boolean {
    return isIdentStart(c) || isDigit(c);
}

function isDigit(c: string): boolean {
    return c >= "0" && c <= "9";
}
