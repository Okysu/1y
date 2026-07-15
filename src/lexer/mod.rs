//! Hand-written lexer for `1y`.
//!
//! Produces a flat [`Vec<Token>`] with full span information. The lexer never
//! panics: on bad input it records a [`SourceError`] and tries to resync at
//! the next plausible token boundary (whitespace / punctuation).
//!
//! Supported lexical features:
//!  - line comments `// ...` and block comments `/* ... */` (nestable)
//!  - numeric separators `1_000_000`
//!  - arbitrary-precision integers (`num-bigint`) and decimals (`bigdecimal`)
//!  - string literals with interpolation `"...{expr}..."` and triple-quoted
//!    multi-line strings `"""..."""` (interpolation source is captured raw
//!    and re-parsed by the parser)

mod token;

pub use token::{Keyword, StrPart, Token, TokenKind};

use crate::ast::{Pos, Span};
use crate::error::SourceError;
use bigdecimal::BigDecimal;
use num_bigint::BigInt;
use num_traits::Num;

/// Output of [`tokenize`]: the token stream plus any recovered errors.
#[derive(Debug, Clone)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<SourceError>,
}

/// Tokenize a source string. Always returns a stream ending in
/// [`TokenKind::Eof`]; check `output.errors` for problems.
pub fn tokenize(src: &str) -> LexOutput {
    let mut lx = Lexer::new(src);
    let mut tokens = Vec::new();
    loop {
        let tok = lx.next_token();
        let is_eof = matches!(tok.kind, TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    LexOutput {
        tokens,
        errors: lx.errors,
    }
}

struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    /// FIFO lookahead buffer: chars read from `chars` but not yet committed.
    /// `peek_buf[0]` is the next char, `peek_buf[1]` the one after, etc.
    peek_buf: Vec<char>,
    pos: Pos,
    errors: Vec<SourceError>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer {
            chars: src.chars(),
            peek_buf: Vec::new(),
            pos: Pos::ZERO,
            errors: Vec::new(),
        }
    }

    // --- cursor helpers (FIFO buffer) ---

    fn fill(&mut self, n: usize) {
        while self.peek_buf.len() < n {
            match self.chars.next() {
                Some(c) => self.peek_buf.push(c),
                None => break,
            }
        }
    }

    /// Peek the next char without consuming.
    fn peek(&mut self) -> Option<char> {
        self.fill(1);
        self.peek_buf.first().copied()
    }

    /// Peek the char after the next.
    fn peek2(&mut self) -> Option<char> {
        self.fill(2);
        self.peek_buf.get(1).copied()
    }

    /// Consume and return the next char, advancing the position.
    fn bump(&mut self) -> Option<char> {
        self.fill(1);
        if let Some(c) = self.peek_buf.first().copied() {
            self.peek_buf.remove(0);
            self.pos = self.pos.advance(c);
            Some(c)
        } else {
            None
        }
    }

    fn error(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(SourceError::new(span, msg));
    }

    fn tok(&self, start: Pos, kind: TokenKind) -> Token {
        Token::new(kind, Span::new(start, self.pos))
    }

    // --- main loop ---

    fn next_token(&mut self) -> Token {
        loop {
            let start = self.pos;
            let c = match self.bump() {
                Some(c) => c,
                None => return Token::new(TokenKind::Eof, Span::at(start)),
            };

            match c {
                ws if ws.is_whitespace() => continue,

                // comments
                '/' if self.peek() == Some('/') => {
                    self.bump();
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
                    continue;
                }
                '/' if self.peek() == Some('*') => {
                    self.bump();
                    self.eat_block_comment(start);
                    continue;
                }

                // identifiers / keywords / wildcard
                id if is_ident_start(id) => {
                    let mut s = String::new();
                    s.push(id);
                    while let Some(ch) = self.peek() {
                        if is_ident_continue(ch) {
                            self.bump();
                            s.push(ch);
                        } else {
                            break;
                        }
                    }
                    let end = self.pos;
                    let kind = if s == "_" {
                        TokenKind::Underscore
                    } else {
                        match Keyword::from_ident(&s) {
                            Some(kw) => TokenKind::Keyword(kw),
                            None => TokenKind::Ident(s),
                        }
                    };
                    return Token::new(kind, Span::new(start, end));
                }

                // numbers
                d if d.is_ascii_digit() => return self.lex_number(start, d),

                // strings
                '"' => return self.lex_string(start),

                // punctuation
                '(' => return self.tok(start, TokenKind::LParen),
                ')' => return self.tok(start, TokenKind::RParen),
                '{' => return self.tok(start, TokenKind::LBrace),
                '}' => return self.tok(start, TokenKind::RBrace),
                '[' => return self.tok(start, TokenKind::LBracket),
                ']' => return self.tok(start, TokenKind::RBracket),
                ',' => return self.tok(start, TokenKind::Comma),
                ';' => return self.tok(start, TokenKind::Semicolon),
                ':' => {
                    if self.peek() == Some(':') {
                        self.bump();
                        return self.tok(start, TokenKind::DoubleColon);
                    }
                    return self.tok(start, TokenKind::Colon);
                }
                '.' => {
                    if self.peek() == Some('.') {
                        self.bump();
                        return self.tok(start, TokenKind::DotDot);
                    }
                    return self.tok(start, TokenKind::Dot);
                }
                '-' => {
                    if self.peek() == Some('>') {
                        self.bump();
                        return self.tok(start, TokenKind::Arrow);
                    }
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::MinusAssign);
                    }
                    return self.tok(start, TokenKind::Minus);
                }
                '=' => {
                    if self.peek() == Some('>') {
                        self.bump();
                        return self.tok(start, TokenKind::FatArrow);
                    }
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::Eq);
                    }
                    return self.tok(start, TokenKind::Assign);
                }
                '!' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::Neq);
                    }
                    return self.tok(start, TokenKind::Bang);
                }
                '?' => return self.tok(start, TokenKind::Question),
                '<' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::Lte);
                    }
                    return self.tok(start, TokenKind::Lt);
                }
                '>' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::Gte);
                    }
                    return self.tok(start, TokenKind::Gt);
                }
                '+' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::PlusAssign);
                    }
                    return self.tok(start, TokenKind::Plus);
                }
                '*' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::StarAssign);
                    }
                    return self.tok(start, TokenKind::Star);
                }
                '/' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::SlashAssign);
                    }
                    return self.tok(start, TokenKind::Slash);
                }
                '%' => {
                    if self.peek() == Some('=') {
                        self.bump();
                        return self.tok(start, TokenKind::PercentAssign);
                    }
                    return self.tok(start, TokenKind::Percent);
                }
                '&' => {
                    if self.peek() == Some('&') {
                        self.bump();
                        return self.tok(start, TokenKind::And);
                    }
                    self.error(
                        Span::new(start, self.pos),
                        "bare `&` is not a valid token (did you mean `&&`?)",
                    );
                    continue;
                }
                '|' => {
                    if self.peek() == Some('|') {
                        self.bump();
                        return self.tok(start, TokenKind::Or);
                    }
                    if self.peek() == Some('>') {
                        self.bump();
                        return self.tok(start, TokenKind::Pipe);
                    }
                    return self.tok(start, TokenKind::Pipe2);
                }
                '#' => {
                    if self.peek() == Some('{') {
                        self.bump();
                        return self.tok(start, TokenKind::HashBrace);
                    }
                    self.error(
                        Span::new(start, self.pos),
                        "bare `#` is not valid (sets are written `#{ ... }`)",
                    );
                    continue;
                }

                other => {
                    self.error(
                        Span::new(start, self.pos),
                        format!("unexpected character `{}`", other),
                    );
                    continue;
                }
            }
        }
    }

    // --- block comments (nestable) ---

    fn eat_block_comment(&mut self, start: Pos) {
        let mut depth: u32 = 1;
        while let Some(ch) = self.bump() {
            match ch {
                '/' if self.peek() == Some('*') => {
                    self.bump();
                    depth += 1;
                }
                '*' if self.peek() == Some('/') => {
                    self.bump();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
        self.error(
            Span::new(start, self.pos),
            "unterminated block comment (missing `*/`)",
        );
    }

    // --- numbers ---

    fn lex_number(&mut self, start: Pos, first: char) -> Token {
        let mut int_part = String::new();
        int_part.push(first);
        while let Some(ch) = self.peek() {
            match ch {
                d if d.is_ascii_digit() => {
                    self.bump();
                    int_part.push(d);
                }
                '_' if self.peek2().map(|c| c.is_ascii_digit()).unwrap_or(false) => {
                    self.bump();
                }
                _ => break,
            }
        }

        // decimal: digit '.' digit
        if self.peek() == Some('.') && self.peek2().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            self.bump(); // '.'
            let mut frac = String::new();
            while let Some(ch) = self.peek() {
                match ch {
                    d if d.is_ascii_digit() => {
                        self.bump();
                        frac.push(d);
                    }
                    '_' if self.peek2().map(|c| c.is_ascii_digit()).unwrap_or(false) => {
                        self.bump();
                    }
                    _ => break,
                }
            }
            let mut literal = format!("{}.{}", int_part, frac);
            if let Some(exp) = self.maybe_exponent() {
                literal.push_str(&exp);
            }
            let end = self.pos;
            let stripped = literal.replace('_', "");
            match stripped.parse::<BigDecimal>() {
                Ok(d) => return Token::new(TokenKind::Decimal(d), Span::new(start, end)),
                Err(_) => {
                    self.error(
                        Span::new(start, end),
                        format!("invalid decimal literal `{}`", stripped),
                    );
                    return Token::new(
                        TokenKind::Decimal(BigDecimal::from(0)),
                        Span::new(start, end),
                    );
                }
            }
        }

        // integer with exponent -> promote to decimal
        if matches!(self.peek(), Some('e') | Some('E')) {
            if let Some(exp) = self.maybe_exponent() {
                let literal = format!("{}{}", int_part, exp);
                let end = self.pos;
                let stripped = literal.replace('_', "");
                match stripped.parse::<BigDecimal>() {
                    Ok(d) => return Token::new(TokenKind::Decimal(d), Span::new(start, end)),
                    Err(_) => {
                        self.error(
                            Span::new(start, end),
                            format!("invalid numeric literal `{}`", stripped),
                        );
                        return Token::new(
                            TokenKind::Decimal(BigDecimal::from(0)),
                            Span::new(start, end),
                        );
                    }
                }
            }
        }

        let end = self.pos;
        let stripped = int_part.replace('_', "");
        match BigInt::from_str_radix(&stripped, 10) {
            Ok(n) => Token::new(TokenKind::Int(n), Span::new(start, end)),
            Err(_) => {
                self.error(
                    Span::new(start, end),
                    format!("invalid integer literal `{}`", stripped),
                );
                Token::new(TokenKind::Int(BigInt::from(0)), Span::new(start, end))
            }
        }
    }

    /// If the next chars form an exponent (`e[+-]?digits`), consume it and
    /// return the literal text (including the `e`). Returns `None` if there
    /// is no `e`/`E`, or records an error and returns the partial text if the
    /// exponent is malformed (so the token still forms).
    fn maybe_exponent(&mut self) -> Option<String> {
        let e = self.peek()?;
        if e != 'e' && e != 'E' {
            return None;
        }
        let saved_pos = self.pos;
        let mut buf = String::new();
        buf.push(self.bump()?); // 'e'/'E'
        match self.peek() {
            Some('+') | Some('-') => {
                buf.push(self.bump().unwrap());
            }
            _ => {}
        }
        let mut digits = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.bump();
                digits.push(ch);
            } else {
                break;
            }
        }
        if digits.is_empty() {
            self.error(
                Span::new(saved_pos, self.pos),
                "expected digits after exponent marker",
            );
            return Some(buf);
        }
        buf.push_str(&digits);
        Some(buf)
    }

    // --- strings ---

    fn lex_string(&mut self, start: Pos) -> Token {
        if self.peek() == Some('"') && self.peek2() == Some('"') {
            self.bump();
            self.bump();
            return self.lex_triple_string(start);
        }

        let mut parts: Vec<StrPart> = Vec::new();
        let mut cur = String::new();

        loop {
            let ch = match self.bump() {
                Some(c) => c,
                None => {
                    self.error(Span::new(start, self.pos), "unterminated string literal");
                    break;
                }
            };
            match ch {
                '"' => break,
                '\\' => {
                    let esc_start = self.pos;
                    let e = match self.bump() {
                        Some(c) => c,
                        None => {
                            self.error(
                                Span::new(esc_start, self.pos),
                                "unterminated escape sequence",
                            );
                            break;
                        }
                    };
                    match e {
                        'n' => cur.push('\n'),
                        't' => cur.push('\t'),
                        'r' => cur.push('\r'),
                        '0' => cur.push('\0'),
                        '\\' => cur.push('\\'),
                        '"' => cur.push('"'),
                        '\'' => cur.push('\''),
                        '{' => cur.push('{'),
                        '}' => cur.push('}'),
                        'u' => self.lex_unicode_escape(esc_start, &mut cur),
                        other => self.error(
                            Span::new(esc_start, self.pos),
                            format!("unknown escape sequence `\\{}`", other),
                        ),
                    }
                }
                '{' => {
                    if !cur.is_empty() {
                        parts.push(StrPart::Literal(std::mem::take(&mut cur)));
                    }
                    let expr_start = self.pos;
                    let raw = self.collect_interpolation(expr_start);
                    parts.push(StrPart::Expr(raw, Span::new(expr_start, self.pos)));
                }
                other => cur.push(other),
            }
        }

        if !cur.is_empty() {
            parts.push(StrPart::Literal(cur));
        }
        if parts.is_empty() {
            parts.push(StrPart::Literal(String::new()));
        }
        let end = self.pos;
        Token::new(TokenKind::Str(parts), Span::new(start, end))
    }

    fn lex_unicode_escape(&mut self, esc_start: Pos, cur: &mut String) {
        if self.peek() != Some('{') {
            self.error(Span::new(esc_start, self.pos), "expected `{` after \\u");
            return;
        }
        self.bump();
        let mut hex = String::new();
        while let Some(h) = self.peek() {
            if h == '}' {
                break;
            }
            if h.is_ascii_hexdigit() {
                self.bump();
                hex.push(h);
            } else {
                self.error(
                    Span::new(self.pos, self.pos),
                    format!("invalid hex digit `{}` in \\u escape", h),
                );
                break;
            }
        }
        if self.peek() == Some('}') {
            self.bump();
        }
        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            Some(c) => cur.push(c),
            None => self.error(
                Span::new(esc_start, self.pos),
                format!("invalid unicode escape \\u{{{}}}", hex),
            ),
        }
    }

    fn lex_triple_string(&mut self, start: Pos) -> Token {
        let mut parts: Vec<StrPart> = Vec::new();
        let mut cur = String::new();

        // Strip a single leading newline right after the opening `"""`.
        if self.peek() == Some('\n') {
            self.bump();
        } else if self.peek() == Some('\r') && self.peek2() == Some('\n') {
            self.bump();
            self.bump();
        }

        loop {
            // closing `"""`?
            if self.peek() == Some('"') && self.peek2() == Some('"') {
                // need a third quote
                self.fill(3);
                if self.peek_buf.get(2) == Some(&'"') {
                    self.bump();
                    self.bump();
                    self.bump();
                    break;
                }
            }
            let ch = match self.bump() {
                Some(c) => c,
                None => {
                    self.error(
                        Span::new(start, self.pos),
                        "unterminated triple-quoted string",
                    );
                    break;
                }
            };
            match ch {
                '\\' => {
                    let esc_start = self.pos;
                    let e = match self.bump() {
                        Some(c) => c,
                        None => break,
                    };
                    match e {
                        'n' => cur.push('\n'),
                        't' => cur.push('\t'),
                        'r' => cur.push('\r'),
                        '\\' => cur.push('\\'),
                        '"' => cur.push('"'),
                        '{' => cur.push('{'),
                        '}' => cur.push('}'),
                        other => self.error(
                            Span::new(esc_start, self.pos),
                            format!("unknown escape sequence `\\{}`", other),
                        ),
                    }
                }
                '{' => {
                    if !cur.is_empty() {
                        parts.push(StrPart::Literal(std::mem::take(&mut cur)));
                    }
                    let expr_start = self.pos;
                    let raw = self.collect_interpolation(expr_start);
                    parts.push(StrPart::Expr(raw, Span::new(expr_start, self.pos)));
                }
                other => cur.push(other),
            }
        }
        if !cur.is_empty() {
            parts.push(StrPart::Literal(cur));
        }
        if parts.is_empty() {
            parts.push(StrPart::Literal(String::new()));
        }
        let end = self.pos;
        Token::new(TokenKind::Str(parts), Span::new(start, end))
    }

    /// Collect the raw source of an interpolation expression, starting just
    /// after the opening `{`. Tracks brace depth so nested `{ ... }` inside
    /// the expression is included, and skips over nested string literals so a
    /// `}` inside a string does not close the interpolation. Stops at the
    /// matching `}` (which is consumed).
    fn collect_interpolation(&mut self, start: Pos) -> String {
        let mut depth: i32 = 1;
        let mut raw = String::new();
        while let Some(ch) = self.bump() {
            match ch {
                '"' => {
                    raw.push('"');
                    self.skip_nested_string_into(&mut raw);
                }
                '{' => {
                    depth += 1;
                    raw.push('{');
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    raw.push('}');
                }
                other => raw.push(other),
            }
        }
        if depth != 0 {
            self.error(
                Span::new(start, self.pos),
                "unterminated interpolation `{` (missing `}`)",
            );
        }
        raw
    }

    /// Copy a nested string literal (the opening `"` already pushed into
    /// `raw`) verbatim, honoring `\\` escapes so the real closing `"` is
    /// found.
    fn skip_nested_string_into(&mut self, raw: &mut String) {
        loop {
            let ch = match self.bump() {
                Some(c) => c,
                None => {
                    self.error(
                        Span::new(self.pos, self.pos),
                        "unterminated string inside interpolation",
                    );
                    return;
                }
            };
            raw.push(ch);
            match ch {
                '\\' => {
                    if let Some(e) = self.bump() {
                        raw.push(e);
                    }
                }
                '"' => return,
                _ => {}
            }
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
