//! Recursive-descent + Pratt parser for `1y`.
//!
//! # Operator precedence (lowest → highest)
//!
//! | Level | Operators                       | Assoc    | Notes                              |
//! |------:|---------------------------------|----------|------------------------------------|
//! |     0 | `=` (assignment)                | right    | returns `nil`; lhs must be lvalue  |
//! |     1 | `\|>` (pipe)                    | left     | `a \|> f(b)` ≡ `f(a, b)`           |
//! |     2 | `!` `?` (actor send / request)  | left     | `c ! msg`, `c ? msg`               |
//! |     3 | `or` `\|\|`                     | left     | short-circuit                      |
//! |     4 | `and` `&&`                      | left     | short-circuit                      |
//! |     5 | `==` `!=` `<` `>` `<=` `>=`     | left     |                                    |
//! |     6 | `+` `-`                         | left     |                                    |
//! |     7 | `*` `/` `%`                     | left     |                                    |
//! |     8 | unary `-` `not` (prefix)        | prefix   |                                    |
//! |     9 | `(` `)` `[` `]` `.` (postfix)   | postfix  | call / index / field / method      |
//! |    10 | primary                         | —        | literals, idents, `(expr)`, blocks |
//!
//! There is **no** `^`/`**` power operator; use the `pow(base, exp)` function
//! from the standard library. Boolean negation is the `not` keyword (the `!`
//! token is reserved for actor send).
//!
//! # Error recovery
//!
//! Parse functions return `Result<T, SourceError>`. The statement / block
//! drivers catch errors, record them, and resynchronize at the next `;`, `}`,
//! or statement-starting keyword, so a whole file is always parsed to
//! completion and produces a list of diagnostics.

use crate::ast::*;
use crate::error::SourceError;
use crate::lexer::{tokenize, Keyword, Token, TokenKind};

/// Output of [`parse`].
#[derive(Debug, Clone)]
pub struct ParseOutput {
    pub program: Program,
    pub errors: Vec<SourceError>,
}

/// Parse a source string into a [`Program`]. Always returns a program (possibly
/// partial); check `output.errors`.
pub fn parse(src: &str) -> ParseOutput {
    let lex = tokenize(src);
    let mut p = Parser::new(lex.tokens);
    let stmts = p.parse_stmt_list(/*top_level*/ true);
    let span = stmts
        .first()
        .map(|s| s.span())
        .unwrap_or_else(Span::dummy);
    let end_span = stmts.last().map(|s| s.span()).unwrap_or(span);
    let program = Program {
        stmts,
        span: span.union(end_span),
    };
    ParseOutput {
        program,
        errors: p.errors,
    }
}

struct Parser {
    tokens: Vec<Token>,
    idx: usize,
    errors: Vec<SourceError>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            idx: 0,
            errors: Vec::new(),
        }
    }

    // --- cursor ---

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.idx].kind
    }

    fn peek_at(&self, n: usize) -> &TokenKind {
        self.tokens
            .get(self.idx + n)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn span_here(&self) -> Span {
        self.tokens[self.idx].span
    }

    fn bump(&mut self) -> &TokenKind {
        let i = self.idx;
        if i + 1 < self.tokens.len() {
            self.idx += 1;
        }
        &self.tokens[i].kind
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn at_kw(&self, kw: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if *k == kw)
    }

    /// If the current token matches `kind`, consume it and return true.
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Keyword) -> bool {
        if self.at_kw(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consume the current keyword or record an error.
    fn expect_kw(&mut self, kw: Keyword, what: &str) -> bool {
        if self.eat_kw(kw) {
            true
        } else {
            let span = self.span_here();
            self.error(
                span,
                format!("expected {}, found {}", what, self.peek().describe()),
            );
            false
        }
    }

    /// Consume the current token if it matches `kind`, else record an error.
    fn expect(&mut self, kind: &TokenKind, what: &str) -> bool {
        if self.eat(kind) {
            true
        } else {
            let span = self.span_here();
            self.error(
                span,
                format!("expected {}, found {}", what, self.peek().describe()),
            );
            false
        }
    }

    fn error(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(SourceError::new(span, msg));
    }

    // --- statement list / blocks ---

    /// Parse a sequence of statements until a terminator (`}` at block level,
    /// EOF at top level).
    fn parse_stmt_list(&mut self, top_level: bool) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            // Skip stray semicolons — e.g. after `let x = 1;` the declaration
            // parser does not consume the trailing `;`, so a leading `;` here
            // is an empty statement and must not be parsed as an expression.
            while matches!(self.peek(), TokenKind::Semicolon) {
                self.bump();
            }
            if self.is_eof() {
                break;
            }
            if !top_level && matches!(self.peek(), TokenKind::RBrace) {
                break;
            }
            match self.parse_stmt() {
                Ok(s) => stmts.push(s),
                Err(e) => {
                    self.errors.push(e);
                    self.resync(top_level);
                }
            }
        }
        stmts
    }

    fn resync(&mut self, top_level: bool) {
        loop {
            match self.peek() {
                TokenKind::Eof => return,
                TokenKind::Semicolon => {
                    self.bump();
                    return;
                }
                TokenKind::RBrace => {
                    if !top_level {
                        return;
                    }
                    self.bump();
                }
                TokenKind::Keyword(k) if is_stmt_start(*k) => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, SourceError> {
        let start = self.span_here();
        match *self.peek() {
            TokenKind::Keyword(Keyword::Let) => self.parse_let(start),
            TokenKind::Keyword(Keyword::Shared) => self.parse_shared(start),
            TokenKind::Keyword(Keyword::Fn) => {
                // `fn name(...)` → func def; `fn(...)` → lambda expr statement.
                if matches!(self.peek_at(1), TokenKind::Ident(_)) {
                    self.parse_func_def(start)
                } else {
                    self.parse_expr_stmt()
                }
            }
            TokenKind::Keyword(Keyword::Type) => self.parse_type_def(start),
            TokenKind::Keyword(Keyword::Enum) => self.parse_enum_def(start),
            TokenKind::Keyword(Keyword::Actor) => self.parse_actor_def(start),
            TokenKind::Keyword(Keyword::Import) => self.parse_import(start, false),
            TokenKind::Keyword(Keyword::Lazy) => self.parse_import(start, true),
            TokenKind::Keyword(Keyword::State) => self.parse_state_decl(start),
            TokenKind::Keyword(Keyword::On) => self.parse_on_clause(start),
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt, SourceError> {
        let expr = self.parse_expr(0)?;
        if self.eat(&TokenKind::Semicolon) {
            Ok(Stmt::Semi(expr))
        } else {
            Ok(Stmt::Expr(expr))
        }
    }

    // --- declarations ---

    fn parse_let(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `let`
        let name = self.expect_ident("let binding name")?;
        let type_annot = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign, "`=`");
        let value = self.parse_expr(0)?;
        Ok(Stmt::Let {
            name,
            type_annot,
            value,
            span: start.union(self.prev_span()),
        })
    }

    fn parse_shared(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `shared`
        let name = self.expect_ident("shared binding name")?;
        let type_annot = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign, "`=`");
        let value = self.parse_expr(0)?;
        Ok(Stmt::SharedDecl {
            name,
            type_annot,
            value,
            span: start.union(self.prev_span()),
        })
    }

    fn parse_state_decl(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `state`
        let name = self.expect_ident("state binding name")?;
        let type_annot = if self.eat(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign, "`=`");
        let value = self.parse_expr(0)?;
        Ok(Stmt::StateDecl {
            name,
            type_annot,
            value,
            span: start.union(self.prev_span()),
        })
    }

    fn parse_func_def(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `fn`
        let name = self.expect_ident("function name")?;
        let params = self.parse_params()?;
        let return_type = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_expr(0)?;
        Ok(Stmt::FuncDef(FuncDef {
            name,
            params,
            return_type,
            body: Box::new(body),
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_on_clause(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `on`
        let name = self.expect_ident("handler name")?;
        let params = self.parse_params()?;
        let return_type = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_expr(0)?;
        Ok(Stmt::OnClause(OnClause {
            name,
            params,
            return_type,
            body: Box::new(body),
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, SourceError> {
        self.expect(&TokenKind::LParen, "`(`");
        let mut params = Vec::new();
        if !matches!(self.peek(), TokenKind::RParen) {
            loop {
                let pspan = self.span_here();
                let name = self.expect_ident("parameter name")?;
                let type_annot = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    type_annot,
                    span: pspan,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`");
        Ok(params)
    }

    fn parse_type_def(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `type`
        let name = self.expect_ident("type name")?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::Assign, "`=`");
        self.expect(&TokenKind::LBrace, "`{`");
        let mut fields = Vec::new();
        if !matches!(self.peek(), TokenKind::RBrace) {
            loop {
                let fname = self.expect_ident("field name")?;
                self.expect(&TokenKind::Colon, "`:`");
                let ftype = self.parse_type()?;
                fields.push((fname, ftype));
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Stmt::TypeDef(TypeDef {
            name,
            type_params,
            fields,
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_enum_def(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `enum`
        let name = self.expect_ident("enum name")?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace, "`{`");
        let mut variants = Vec::new();
        if !matches!(self.peek(), TokenKind::RBrace) {
            loop {
                let vspan = self.span_here();
                let vname = self.expect_ident("variant name")?;
                let mut fields = Vec::new();
                if self.eat(&TokenKind::LParen) {
                    if !matches!(self.peek(), TokenKind::RParen) {
                        loop {
                            fields.push(self.parse_type()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::RParen, "`)`");
                }
                variants.push(VariantDef {
                    name: vname,
                    fields,
                    span: vspan,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Stmt::EnumDef(EnumDef {
            name,
            type_params,
            variants,
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_actor_def(&mut self, start: Span) -> Result<Stmt, SourceError> {
        self.bump(); // `actor`
        let name = self.expect_ident("actor name")?;
        self.expect(&TokenKind::LBrace, "`{`");
        // Actor body: restricted statement list (state / on / fn). We reuse the
        // block-style parser (terminator `}`).
        let body = self.parse_stmt_list(false);
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Stmt::ActorDef(ActorDef {
            name,
            body,
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_import(&mut self, start: Span, lazy: bool) -> Result<Stmt, SourceError> {
        if lazy {
            self.bump(); // `lazy`
            self.expect(&TokenKind::Keyword(Keyword::Import), "`import`");
        } else {
            self.bump(); // `import`
        }
        // dotted path
        let mut path = self.expect_ident("module path")?;
        while self.eat(&TokenKind::Dot) {
            path.push('.');
            path.push_str(&self.expect_ident("module path segment")?);
        }
        let alias = if self.eat_kw(Keyword::As) {
            Some(self.expect_ident("alias name")?)
        } else {
            None
        };
        Ok(Stmt::Import(Import {
            path,
            alias,
            lazy,
            span: start.union(self.prev_span()),
        }))
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>, SourceError> {
        let mut tps = Vec::new();
        if self.eat(&TokenKind::Lt) {
            if !matches!(self.peek(), TokenKind::Gt) {
                loop {
                    tps.push(self.expect_ident("type parameter")?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::Gt, "`>`");
        }
        Ok(tps)
    }

    // --- type annotations ---

    fn parse_type(&mut self) -> Result<TypeAnnot, SourceError> {
        // function type: `fn(T, U) -> R`
        if self.eat_kw(Keyword::Fn) {
            self.expect(&TokenKind::LParen, "`(`");
            let mut params = Vec::new();
            if !matches!(self.peek(), TokenKind::RParen) {
                loop {
                    params.push(self.parse_type()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::RParen, "`)`");
            self.expect(&TokenKind::Arrow, "`->`");
            let ret = self.parse_type()?;
            let span = self.prev_span();
            return Ok(TypeAnnot::Fn {
                params,
                ret: Box::new(ret),
                span,
            });
        }

        // atomic: name or name<...>
        let start = self.span_here();
        let name = self.expect_ident("type name")?;
        let mut ty = if self.eat(&TokenKind::Lt) {
            let mut args = Vec::new();
            if !matches!(self.peek(), TokenKind::Gt) {
                loop {
                    args.push(self.parse_type()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            }
            self.expect(&TokenKind::Gt, "`>`");
            TypeAnnot::Generic {
                name,
                args,
                span: start.union(self.prev_span()),
            }
        } else {
            TypeAnnot::Name {
                name,
                span: start,
            }
        };

        // union types: `A | B`
        if self.peek_is_pipe2() {
            let mut variants = vec![ty];
            while self.peek_is_pipe2() {
                self.bump();
                let s = self.span_here();
                let n = self.expect_ident("type name")?;
                let v = if self.eat(&TokenKind::Lt) {
                    let mut args = Vec::new();
                    if !matches!(self.peek(), TokenKind::Gt) {
                        loop {
                            args.push(self.parse_type()?);
                            if !self.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&TokenKind::Gt, "`>`");
                    TypeAnnot::Generic {
                        name: n,
                        args,
                        span: s.union(self.prev_span()),
                    }
                } else {
                    TypeAnnot::Name { name: n, span: s }
                };
                variants.push(v);
            }
            let span = variants
                .first()
                .unwrap()
                .span()
                .union(variants.last().unwrap().span());
            ty = TypeAnnot::Union { variants, span };
        }
        Ok(ty)
    }

    fn peek_is_pipe2(&self) -> bool {
        matches!(self.peek(), TokenKind::Pipe2)
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, SourceError> {
        match self.peek().clone() {
            TokenKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => {
                let span = self.span_here();
                Err(SourceError::new(
                    span,
                    format!("expected {}, found {}", what, other.describe()),
                ))
            }
        }
    }

    /// Span of the most recently consumed token.
    fn prev_span(&self) -> Span {
        if self.idx == 0 {
            return self.tokens[0].span;
        }
        self.tokens[self.idx - 1].span
    }

    // --- expressions (Pratt) ---

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, SourceError> {
        let mut lhs = self.parse_prefix()?;

        loop {
            // assignment (lowest precedence, right-associative). Only triggers
            // at the top of an expression (min_bp == 0); rhs parsed at min_bp
            // 0 so `a = b = c` nests as `a = (b = c)`.
            if min_bp == 0 && matches!(self.peek(), TokenKind::Assign) {
                let span = self.span_here();
                self.bump();
                let rhs = self.parse_expr(0)?;
                lhs = Expr::Assign {
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                    span: span.union(self.prev_span()),
                };
                continue;
            }

            // compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`)
            if min_bp == 0 {
                let cop = match self.peek() {
                    TokenKind::PlusAssign => Some(BinOp::Add),
                    TokenKind::MinusAssign => Some(BinOp::Sub),
                    TokenKind::StarAssign => Some(BinOp::Mul),
                    TokenKind::SlashAssign => Some(BinOp::Div),
                    TokenKind::PercentAssign => Some(BinOp::Mod),
                    _ => None,
                };
                if let Some(op) = cop {
                    let span = self.span_here();
                    self.bump();
                    let rhs = self.parse_expr(0)?;
                    lhs = Expr::CompoundAssign {
                        op,
                        target: Box::new(lhs),
                        value: Box::new(rhs),
                        span: span.union(self.prev_span()),
                    };
                    continue;
                }
            }

            let (lbp, rbp, op) = match self.infix_op() {
                Some(x) => x,
                None => break,
            };
            if lbp < min_bp {
                break;
            }
            let op_span = self.span_here();
            self.bump();
            let rhs = self.parse_expr(rbp)?;
            // Capture spans BEFORE moving lhs/rhs into Box::new(...).
            let lhs_span = lhs.span();
            let rhs_span = rhs.span();
            lhs = match op {
                InfixOp::Pipe => Expr::Pipe {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span: lhs_span.union(rhs_span),
                },
                InfixOp::ActorSend => Expr::ActorSend {
                    actor: Box::new(lhs),
                    msg: Box::new(rhs),
                    span: lhs_span.union(rhs_span),
                },
                InfixOp::ActorRequest => Expr::ActorRequest {
                    actor: Box::new(lhs),
                    msg: Box::new(rhs),
                    span: lhs_span.union(rhs_span),
                },
                InfixOp::Bin(b) => Expr::BinOp {
                    op: b,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span: lhs_span.union(rhs_span),
                },
            };
            let _ = op_span;
        }

        Ok(lhs)
    }

    /// If the current token is an infix operator (excluding assignment, which
    /// is handled separately), return its `(left_bp, right_bp, kind)`.
    fn infix_op(&self) -> Option<(u8, u8, InfixOp)> {
        let (lbp, rbp) = match self.peek() {
            TokenKind::Pipe => (1, 2),                 // `|>` left
            TokenKind::Bang => (3, 4),                 // `!`  actor send, left
            TokenKind::Question => (3, 4),             // `?`  actor request, left
            TokenKind::Keyword(Keyword::Or) => (5, 6), // `or`, left
            TokenKind::Or => (5, 6),                   // `||`, left
            TokenKind::Keyword(Keyword::And) => (7, 8),// `and`, left
            TokenKind::And => (7, 8),                  // `&&`, left
            TokenKind::Eq => (9, 10),
            TokenKind::Neq => (9, 10),
            TokenKind::Lt => (9, 10),
            TokenKind::Gt => (9, 10),
            TokenKind::Lte => (9, 10),
            TokenKind::Gte => (9, 10),
            TokenKind::Plus => (11, 12),
            TokenKind::Minus => (11, 12),
            TokenKind::Star => (13, 14),
            TokenKind::Slash => (13, 14),
            TokenKind::Percent => (13, 14),
            _ => return None,
        };
        let op = match self.peek() {
            TokenKind::Pipe => InfixOp::Pipe,
            TokenKind::Bang => InfixOp::ActorSend,
            TokenKind::Question => InfixOp::ActorRequest,
            TokenKind::Keyword(Keyword::Or) | TokenKind::Or => InfixOp::Bin(BinOp::Or),
            TokenKind::Keyword(Keyword::And) | TokenKind::And => InfixOp::Bin(BinOp::And),
            TokenKind::Eq => InfixOp::Bin(BinOp::Eq),
            TokenKind::Neq => InfixOp::Bin(BinOp::Neq),
            TokenKind::Lt => InfixOp::Bin(BinOp::Lt),
            TokenKind::Gt => InfixOp::Bin(BinOp::Gt),
            TokenKind::Lte => InfixOp::Bin(BinOp::Lte),
            TokenKind::Gte => InfixOp::Bin(BinOp::Gte),
            TokenKind::Plus => InfixOp::Bin(BinOp::Add),
            TokenKind::Minus => InfixOp::Bin(BinOp::Sub),
            TokenKind::Star => InfixOp::Bin(BinOp::Mul),
            TokenKind::Slash => InfixOp::Bin(BinOp::Div),
            TokenKind::Percent => InfixOp::Bin(BinOp::Mod),
            _ => unreachable!(),
        };
        Some((lbp, rbp, op))
    }

    fn parse_prefix(&mut self) -> Result<Expr, SourceError> {
        let start = self.span_here();
        // unary prefix operators
        match self.peek() {
            TokenKind::Minus => {
                self.bump();
                let operand = self.parse_expr(15)?;
                return Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(operand),
                    span: start.union(self.prev_span()),
                });
            }
            TokenKind::Keyword(Keyword::Not) => {
                self.bump();
                let operand = self.parse_expr(15)?;
                return Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(operand),
                    span: start.union(self.prev_span()),
                });
            }
            _ => {}
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, SourceError> {
        let mut lhs = self.parse_primary()?;
        loop {
            match self.peek() {
                TokenKind::LParen => {
                    let start = lhs.span();
                    self.bump();
                    let args = self.parse_args()?;
                    lhs = Expr::Call {
                        callee: Box::new(lhs),
                        args,
                        span: start.union(self.prev_span()),
                    };
                }
                TokenKind::LBracket => {
                    let start = lhs.span();
                    self.bump();
                    let index = self.parse_expr(0)?;
                    self.expect(&TokenKind::RBracket, "`]`");
                    lhs = Expr::Index {
                        target: Box::new(lhs),
                        index: Box::new(index),
                        span: start.union(self.prev_span()),
                    };
                }
                TokenKind::Dot => {
                    let start = lhs.span();
                    self.bump();
                    let name = self.expect_ident("field or method name")?;
                    if matches!(self.peek(), TokenKind::LParen) {
                        self.bump();
                        let args = self.parse_args()?;
                        lhs = Expr::MethodCall {
                            receiver: Box::new(lhs),
                            method: name,
                            args,
                            span: start.union(self.prev_span()),
                        };
                    } else {
                        lhs = Expr::Field {
                            target: Box::new(lhs),
                            name,
                            span: start.union(self.prev_span()),
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, SourceError> {
        let mut args = Vec::new();
        if !matches!(self.peek(), TokenKind::RParen) {
            loop {
                args.push(self.parse_expr(0)?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`");
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, SourceError> {
        let start = self.span_here();
        let tok = self.peek().clone();
        match tok {
            TokenKind::Int(n) => {
                self.bump();
                Ok(Expr::Int(n, start))
            }
            TokenKind::Decimal(d) => {
                self.bump();
                Ok(Expr::Decimal(d, start))
            }
            TokenKind::Str(parts) => {
                self.bump();
                self.build_string(parts, start)
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Expr::Bool(true, start))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Expr::Bool(false, start))
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.bump();
                Ok(Expr::Nil(start))
            }
            TokenKind::Ident(name) => {
                self.bump();
                Ok(Expr::Ident(name, start))
            }

            // parenthesised expression
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr(0)?;
                self.expect(&TokenKind::RParen, "`)`");
                Ok(Expr::Paren(Box::new(inner), start.union(self.prev_span())))
            }

            // vector literal
            TokenKind::LBracket => {
                self.bump();
                let mut items = Vec::new();
                if !matches!(self.peek(), TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr(0)?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "`]`");
                Ok(Expr::VecLit {
                    items,
                    span: start.union(self.prev_span()),
                })
            }

            // set literal
            TokenKind::HashBrace => {
                self.bump();
                let mut items = Vec::new();
                if !matches!(self.peek(), TokenKind::RBrace) {
                    loop {
                        items.push(self.parse_expr(0)?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBrace, "`}`");
                Ok(Expr::SetLit {
                    items,
                    span: start.union(self.prev_span()),
                })
            }

            // block `{ ... }` or map literal `{ k: v, ... }`
            TokenKind::LBrace => self.parse_brace_expr(start),

            // control-flow keywords as expression primaries
            TokenKind::Keyword(Keyword::If) => self.parse_if(start),
            TokenKind::Keyword(Keyword::Match) => self.parse_match(start),
            TokenKind::Keyword(Keyword::Fn) => self.parse_lambda(start),
            TokenKind::Keyword(Keyword::Raise) => {
                self.bump();
                let e = self.parse_expr(0)?;
                Ok(Expr::Raise {
                    expr: Box::new(e),
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.bump();
                let value = if matches!(
                    self.peek(),
                    TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof | TokenKind::Comma
                ) {
                    None
                } else {
                    Some(Box::new(self.parse_expr(0)?))
                };
                Ok(Expr::Return {
                    value,
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Reply) => {
                self.bump();
                let e = self.parse_expr(0)?;
                Ok(Expr::Reply {
                    value: Box::new(e),
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Transact) => {
                self.bump();
                let body = self.parse_expr(0)?;
                Ok(Expr::Transact {
                    body: Box::new(body),
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Retry) => {
                self.bump();
                Ok(Expr::Retry {
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Try) => self.parse_try(start),
            TokenKind::Keyword(Keyword::While) => self.parse_while(start),
            TokenKind::Keyword(Keyword::For) => self.parse_for(start),
            TokenKind::Keyword(Keyword::Loop) => {
                self.bump();
                let body = self.parse_expr(0)?;
                Ok(Expr::Loop {
                    body: Box::new(body),
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Break) => {
                self.bump();
                let value = if matches!(
                    self.peek(),
                    TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof | TokenKind::Comma
                ) {
                    None
                } else {
                    Some(Box::new(self.parse_expr(0)?))
                };
                Ok(Expr::Break {
                    value,
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.bump();
                Ok(Expr::Continue {
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::Keyword(Keyword::Spawn) => {
                self.bump();
                let name = self.expect_ident("actor name after `spawn`")?;
                self.expect(&TokenKind::LParen, "`(`");
                let args = self.parse_args()?;
                Ok(Expr::Spawn {
                    name,
                    args,
                    span: start.union(self.prev_span()),
                })
            }

            other => Err(SourceError::new(
                start,
                format!("unexpected {} in expression", other.describe()),
            )),
        }
    }

    /// Parse `{ ... }` which is either a block or a map literal, decided by
    /// lookahead: after `{`, an empty `}` is an empty block; otherwise we parse
    /// one expression and if a `:` follows it is a map, else a block.
    fn parse_brace_expr(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `{`

        // empty block
        if matches!(self.peek(), TokenKind::RBrace) {
            self.bump();
            return Ok(Expr::Block {
                stmts: vec![],
                tail: None,
                span: start.union(self.prev_span()),
            });
        }

        // declarations can't start a map, so a leading decl keyword ⇒ block
        if matches!(self.peek(), TokenKind::Keyword(k) if is_decl_start(*k)) {
            return self.finish_block(start);
        }

        // parse one expression (potential first map key)
        let first = self.parse_expr(0)?;
        if matches!(self.peek(), TokenKind::Colon) {
            // map literal
            self.bump(); // `:`
            let v = self.parse_expr(0)?;
            let mut entries = vec![(first, v)];
            while self.eat(&TokenKind::Comma) {
                if matches!(self.peek(), TokenKind::RBrace) {
                    break; // trailing comma
                }
                let k = self.parse_expr(0)?;
                self.expect(&TokenKind::Colon, "`:`");
                let val = self.parse_expr(0)?;
                entries.push((k, val));
            }
            self.expect(&TokenKind::RBrace, "`}`");
            return Ok(Expr::MapLit {
                entries,
                span: start.union(self.prev_span()),
            });
        }

        // block: `first` is either a Semi stmt or the tail
        let mut stmts = Vec::new();
        let tail;
        if self.eat(&TokenKind::Semicolon) {
            stmts.push(Stmt::Semi(first));
            tail = self.continue_block_stmts(&mut stmts)?;
        } else if matches!(self.peek(), TokenKind::RBrace) {
            tail = Some(Box::new(first));
        } else {
            // not `;` and not `}`: error, but treat first as tail to recover
            self.error(
                self.span_here(),
                format!(
                    "expected `;` or `}}` in block, found {}",
                    self.peek().describe()
                ),
            );
            tail = Some(Box::new(first));
        }
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Expr::Block {
            stmts,
            tail,
            span: start.union(self.prev_span()),
        })
    }

    fn finish_block(&mut self, start: Span) -> Result<Expr, SourceError> {
        let mut stmts = Vec::new();
        let tail = self.continue_block_stmts(&mut stmts)?;
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Expr::Block {
            stmts,
            tail,
            span: start.union(self.prev_span()),
        })
    }

    /// Continue parsing block statements until `}`. Returns the optional tail
    /// expression (the last non-`;`-terminated expression).
    fn continue_block_stmts(&mut self, stmts: &mut Vec<Stmt>) -> Result<Option<Box<Expr>>, SourceError> {
        loop {
            if matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            match self.parse_stmt() {
                Ok(Stmt::Expr(e)) => {
                    // expression without trailing `;` ⇒ tail
                    return Ok(Some(Box::new(e)));
                }
                Ok(s) => {
                    // declarations and `;`-terminated exprs consume an optional
                    // trailing `;` (already consumed for Semi; decls may have one)
                    if matches!(self.peek(), TokenKind::Semicolon) {
                        self.bump();
                    }
                    stmts.push(s);
                }
                Err(e) => {
                    self.errors.push(e);
                    self.resync(false);
                }
            }
        }
        Ok(None)
    }

    fn parse_if(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `if`
        let cond = self.parse_expr(0)?;
        let then = self.parse_expr(0)?;
        let else_ = if self.eat_kw(Keyword::Else) {
            if self.at_kw(Keyword::If) {
                Some(Box::new(self.parse_if(self.span_here())?))
            } else {
                Some(Box::new(self.parse_expr(0)?))
            }
        } else {
            None
        };
        Ok(Expr::If {
            cond: Box::new(cond),
            then: Box::new(then),
            else_,
            span: start.union(self.prev_span()),
        })
    }

    /// `while cond { body }`
    fn parse_while(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `while`
        let cond = self.parse_expr(0)?;
        let body = self.parse_expr(0)?;
        Ok(Expr::While {
            cond: Box::new(cond),
            body: Box::new(body),
            span: start.union(self.prev_span()),
        })
    }

    /// `for var in iter { body }`
    fn parse_for(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `for`
        let var = self.expect_ident("loop variable after `for`")?;
        self.expect_kw(Keyword::In, "`in`");
        let iter = self.parse_expr(0)?;
        let body = self.parse_expr(0)?;
        Ok(Expr::For {
            var,
            iter: Box::new(iter),
            body: Box::new(body),
            span: start.union(self.prev_span()),
        })
    }

    fn parse_match(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `match`
        let scrutinee = self.parse_expr(0)?;
        self.expect(&TokenKind::LBrace, "`{`");
        let mut arms = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let arm_start = self.span_here();
            let pattern = self.parse_pattern()?;
            let guard = if self.eat_kw(Keyword::If) {
                Some(Box::new(self.parse_expr(0)?))
            } else {
                None
            };
            self.expect(&TokenKind::FatArrow, "`=>`");
            let body = self.parse_expr(0)?;
            arms.push(MatchArm {
                pattern,
                guard,
                body: Box::new(body),
                span: arm_start.union(self.prev_span()),
            });
            if !self.eat(&TokenKind::Comma) {
                // allow arm bodies that are blocks to omit the comma
                if matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RBrace, "`}`");
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: start.union(self.prev_span()),
        })
    }

    fn parse_lambda(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `fn`
        self.expect(&TokenKind::LParen, "`(`");
        let mut params = Vec::new();
        if !matches!(self.peek(), TokenKind::RParen) {
            loop {
                let pspan = self.span_here();
                let name = self.expect_ident("parameter name")?;
                let type_annot = if self.eat(&TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(Param {
                    name,
                    type_annot,
                    span: pspan,
                });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`");
        let return_type = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_expr(0)?;
        Ok(Expr::Lambda {
            params,
            return_type,
            body: Box::new(body),
            span: start.union(self.prev_span()),
        })
    }

    fn parse_try(&mut self, start: Span) -> Result<Expr, SourceError> {
        self.bump(); // `try`
        let body = self.parse_expr(0)?;
        let mut rescues = Vec::new();
        while self.at_kw(Keyword::Rescue) {
            let rspan = self.span_here();
            self.bump();
            let type_name = if matches!(self.peek(), TokenKind::Ident(_)) {
                Some(self.expect_ident("exception type name")?)
            } else {
                None
            };
            let bind = if self.eat_kw(Keyword::As) {
                Some(self.expect_ident("binding name")?)
            } else {
                None
            };
            let rbody = self.parse_expr(0)?;
            rescues.push(RescueClause {
                type_name,
                bind,
                body: Box::new(rbody),
                span: rspan.union(self.prev_span()),
            });
        }
        let ensure = if self.eat_kw(Keyword::Ensure) {
            Some(Box::new(self.parse_expr(0)?))
        } else {
            None
        };
        Ok(Expr::Try {
            body: Box::new(body),
            rescues,
            ensure,
            span: start.union(self.prev_span()),
        })
    }

    // --- patterns ---

    fn parse_pattern(&mut self) -> Result<Pattern, SourceError> {
        let first = self.parse_pattern_atomic()?;
        if self.peek_is_pipe2() {
            let mut alts = vec![first];
            while self.peek_is_pipe2() {
                self.bump();
                alts.push(self.parse_pattern_atomic()?);
            }
            let span = alts
                .first()
                .unwrap()
                .pattern_span()
                .union(alts.last().unwrap().pattern_span());
            Ok(Pattern::Or(alts, span))
        } else {
            Ok(first)
        }
    }

    fn parse_pattern_atomic(&mut self) -> Result<Pattern, SourceError> {
        let start = self.span_here();
        match self.peek().clone() {
            TokenKind::Underscore => {
                self.bump();
                Ok(Pattern::Wildcard(start))
            }
            TokenKind::Int(n) => {
                self.bump();
                Ok(Pattern::Lit(LitPattern::Int(n), start))
            }
            TokenKind::Decimal(d) => {
                self.bump();
                Ok(Pattern::Lit(LitPattern::Decimal(d), start))
            }
            TokenKind::Str(parts) => {
                self.bump();
                let lit = self.build_string_parts(parts)?;
                Ok(Pattern::Lit(LitPattern::Str(lit.parts), start))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Pattern::Lit(LitPattern::Bool(true), start))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Pattern::Lit(LitPattern::Bool(false), start))
            }
            TokenKind::Keyword(Keyword::Nil) => {
                self.bump();
                Ok(Pattern::Lit(LitPattern::Nil, start))
            }
            TokenKind::Minus if matches!(self.peek_at(1), TokenKind::Int(_) | TokenKind::Decimal(_)) => {
                // negative numeric literal pattern
                self.bump();
                match self.peek().clone() {
                    TokenKind::Int(n) => {
                        self.bump();
                        Ok(Pattern::Lit(LitPattern::Int(-n), start))
                    }
                    TokenKind::Decimal(d) => {
                        self.bump();
                        Ok(Pattern::Lit(LitPattern::Decimal(-d), start))
                    }
                    _ => unreachable!(),
                }
            }
            TokenKind::Ident(name) => {
                self.bump();
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    // struct pattern: `Name { field: pat, ... }`
                    if matches!(self.peek(), TokenKind::LBrace) {
                        self.bump(); // `{`
                        let mut fields = Vec::new();
                        let mut rest = false;
                        if !matches!(self.peek(), TokenKind::RBrace) {
                            loop {
                                if self.eat(&TokenKind::DotDot) {
                                    rest = true;
                                    break;
                                }
                                let fname = self.expect_ident("field name in struct pattern")?;
                                let fpat = if self.eat(&TokenKind::Colon) {
                                    self.parse_pattern()?
                                } else {
                                    // shorthand `{ x }` ⇒ binds `x`
                                    Pattern::Bind(fname.clone(), start)
                                };
                                fields.push((fname, fpat));
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RBrace, "`}`");
                        return Ok(Pattern::Struct {
                            fields,
                            rest,
                            span: start.union(self.prev_span()),
                        });
                    }
                    // variant pattern: `Name` or `Name(args)`
                    let mut args = Vec::new();
                    if self.eat(&TokenKind::LParen) {
                        if !matches!(self.peek(), TokenKind::RParen) {
                            loop {
                                args.push(self.parse_pattern()?);
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RParen, "`)`");
                    }
                    Ok(Pattern::Variant {
                        name,
                        args,
                        span: start.union(self.prev_span()),
                    })
                } else {
                    Ok(Pattern::Bind(name, start))
                }
            }
            TokenKind::LBracket => {
                self.bump();
                let mut pats = Vec::new();
                let mut rest = false;
                if !matches!(self.peek(), TokenKind::RBracket) {
                    loop {
                        if self.eat(&TokenKind::DotDot) {
                            rest = true;
                            // optionally bind the rest: `..rest`
                            if matches!(self.peek(), TokenKind::Ident(_)) {
                                // bind name ignored in Phase 0 (just mark rest)
                                self.bump();
                            }
                            break;
                        }
                        pats.push(self.parse_pattern()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket, "`]`");
                Ok(Pattern::Vec {
                    pats,
                    rest,
                    span: start.union(self.prev_span()),
                })
            }
            TokenKind::LBrace => {
                self.bump();
                let mut fields = Vec::new();
                let mut rest = false;
                if !matches!(self.peek(), TokenKind::RBrace) {
                    loop {
                        if self.eat(&TokenKind::DotDot) {
                            rest = true;
                            break;
                        }
                        let fname = self.expect_ident("field name in struct pattern")?;
                        let fpat = if self.eat(&TokenKind::Colon) {
                            self.parse_pattern()?
                        } else {
                            // shorthand `{ x }` ⇒ binds `x`
                            Pattern::Bind(fname.clone(), start)
                        };
                        fields.push((fname, fpat));
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBrace, "`}`");
                Ok(Pattern::Struct {
                    fields,
                    rest,
                    span: start.union(self.prev_span()),
                })
            }
            other => Err(SourceError::new(
                start,
                format!("unexpected {} in pattern", other.describe()),
            )),
        }
    }

    // --- string interpolation: re-parse captured raw source ---
    //
    // The lexer captured each interpolation as a raw source string (see
    // [`crate::lexer::StrPart::Expr`]). Here we re-tokenize and parse that raw
    // slice into a full [`Expr`]. The resulting `ast::StrPart::Expr` carries
    // the parsed expression; the outer string node holds the ordered list.

    fn build_string(
        &mut self,
        parts: Vec<crate::lexer::StrPart>,
        start: Span,
    ) -> Result<Expr, SourceError> {
        let lit = self.build_string_parts(parts)?;
        Ok(Expr::Str(lit, start))
    }

    fn build_string_parts(
        &mut self,
        parts: Vec<crate::lexer::StrPart>,
    ) -> Result<StrLit, SourceError> {
        let mut out: Vec<StrPart> = Vec::with_capacity(parts.len());
        for p in parts {
            match p {
                crate::lexer::StrPart::Literal(s) => out.push(StrPart::Literal(s)),
                crate::lexer::StrPart::Expr(raw, span) => {
                    let expr = parse_interpolation(&raw, span).map_err(|mut e| {
                        // surface with the interpolation span if not already set
                        if e.span.start.offset == 0 && e.span.end.offset == 0 {
                            e.span = span;
                        }
                        e
                    })?;
                    out.push(StrPart::Expr(expr));
                }
            }
        }
        Ok(StrLit { parts: out })
    }
}

/// Parse the raw source of an interpolation expression into an [`Expr`].
fn parse_interpolation(raw: &str, _span: Span) -> Result<Expr, SourceError> {
    let lex = tokenize(raw);
    if !lex.errors.is_empty() {
        return Err(lex.errors[0].clone());
    }
    let mut p = Parser::new(lex.tokens);
    let expr = p.parse_expr(0)?;
    if !matches!(p.peek(), TokenKind::Eof) {
        return Err(SourceError::new(
            p.span_here(),
            format!(
                "unexpected trailing tokens in interpolation: {}",
                p.peek().describe()
            ),
        ));
    }
    Ok(expr)
}

// --- helpers ---

#[derive(Debug, Clone, Copy)]
enum InfixOp {
    Pipe,
    ActorSend,
    ActorRequest,
    Bin(BinOp),
}

fn is_stmt_start(k: Keyword) -> bool {
    matches!(
        k,
        Keyword::Let
            | Keyword::Shared
            | Keyword::Fn
            | Keyword::Type
            | Keyword::Enum
            | Keyword::Actor
            | Keyword::Import
            | Keyword::Lazy
            | Keyword::State
            | Keyword::On
    )
}

fn is_decl_start(k: Keyword) -> bool {
    is_stmt_start(k)
}

/// Helper trait so pattern-span unions compile without forcing every variant
/// to expose `span()` through the main enum (kept private to the ast module).
trait PatternSpan {
    fn pattern_span(&self) -> Span;
}

impl PatternSpan for Pattern {
    fn pattern_span(&self) -> Span {
        match self {
            Pattern::Wildcard(s)
            | Pattern::Lit(_, s)
            | Pattern::Bind(_, s)
            | Pattern::Variant { span: s, .. }
            | Pattern::Struct { span: s, .. }
            | Pattern::Vec { span: s, .. }
            | Pattern::Or(_, s)
            | Pattern::Guard(_, _, s) => *s,
        }
    }
}
