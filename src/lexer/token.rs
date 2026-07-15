//! Token definitions for the `1y` lexer.

use crate::ast::Span;
use bigdecimal::BigDecimal;
use num_bigint::BigInt;

/// A token together with its source span.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}

/// The kind of a single token.
///
/// `Int`/`Decimal` carry their parsed numeric values directly so the parser
/// does not have to re-scan digits. String literals are returned as a list of
/// [`StrPart`] so that interpolation is already split out.
#[derive(Debug, Clone)]
pub enum TokenKind {
    // --- Literals ---
    Int(BigInt),
    Decimal(BigDecimal),
    Str(Vec<StrPart>),

    // --- Identifiers & keywords ---
    Ident(String),
    Keyword(Keyword),

    // --- Punctuation ---
    // single / multi-char punctuation are kept as distinct variants for
    // exhaustive matching in the parser.
    LParen,      // (
    RParen,      // )
    LBrace,      // {
    RBrace,      // }
    LBracket,    // [
    RBracket,    // ]
    HashBrace,   // #{
    Comma,       // ,
    Colon,       // :
    DoubleColon, // ::
    Semicolon,   // ;
    Dot,         // .
    DotDot,      // ..  (rest pattern)
    Arrow,       // ->
    FatArrow,    // =>
    Pipe,        // |>
    Bang,        // !  (actor send; boolean negation uses `not`)
    Question,    // ?  (actor request)
    Assign,      // =
    Eq,          // ==
    Neq,         // !=
    Lt,          // <
    Gt,          // >
    Lte,         // <=
    Gte,         // >=
    Plus,        // +
    Minus,       // -
    Star,        // *
    Slash,       // /
    Percent,     // %
    PlusAssign,  // +=
    MinusAssign, // -=
    StarAssign,  // *=
    SlashAssign, // /=
    PercentAssign,// %=
    And,         // &&
    Or,          // ||
    Pipe2,       // |   (used for or-patterns / union types)
    Underscore,  // _  (wildcard)

    Eof,
}

/// One piece of an interpolated string literal.
#[derive(Debug, Clone)]
pub enum StrPart {
    /// A literal run of characters.
    Literal(String),
    /// The raw source of an interpolation expression `{ ... }`. The parser
    /// re-lexes/parses this into an [`crate::ast::Expr`] later.
    Expr(String, Span),
}

/// All reserved keywords. Keeping them in an enum makes the parser's
/// `match` exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keyword {
    // bindings / control flow
    Let,
    Fn,
    If,
    Else,
    Match,
    While,
    For,
    In,
    Break,
    Continue,
    Loop,
    // types
    Enum,
    Type,
    Struct,
    // actor / concurrency
    Actor,
    State,
    On,
    Spawn,
    Reply,
    Yield,
    Await,
    Shared,
    Transact,
    Retry,
    // modules
    Import,
    Lazy,
    As,
    // exceptions
    Raise,
    Try,
    Rescue,
    Ensure,
    Return,
    // literals
    True,
    False,
    Nil,
    // logical
    And,
    Or,
    Not,
}

impl Keyword {
    /// Lookup a keyword from an identifier string, returning `None` if it is
    /// not reserved.
    pub fn from_ident(ident: &str) -> Option<Keyword> {
        Some(match ident {
            "let" => Keyword::Let,
            "fn" => Keyword::Fn,
            "if" => Keyword::If,
            "else" => Keyword::Else,
            "match" => Keyword::Match,
            "while" => Keyword::While,
            "for" => Keyword::For,
            "in" => Keyword::In,
            "break" => Keyword::Break,
            "continue" => Keyword::Continue,
            "loop" => Keyword::Loop,
            "enum" => Keyword::Enum,
            "type" => Keyword::Type,
            "struct" => Keyword::Struct,
            "actor" => Keyword::Actor,
            "state" => Keyword::State,
            "on" => Keyword::On,
            "spawn" => Keyword::Spawn,
            "reply" => Keyword::Reply,
            "yield" => Keyword::Yield,
            "await" => Keyword::Await,
            "shared" => Keyword::Shared,
            "transact" => Keyword::Transact,
            "retry" => Keyword::Retry,
            "import" => Keyword::Import,
            "lazy" => Keyword::Lazy,
            "as" => Keyword::As,
            "raise" => Keyword::Raise,
            "try" => Keyword::Try,
            "rescue" => Keyword::Rescue,
            "ensure" => Keyword::Ensure,
            "return" => Keyword::Return,
            "true" => Keyword::True,
            "false" => Keyword::False,
            "nil" => Keyword::Nil,
            "and" => Keyword::And,
            "or" => Keyword::Or,
            "not" => Keyword::Not,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Let => "let",
            Keyword::Fn => "fn",
            Keyword::If => "if",
            Keyword::Else => "else",
            Keyword::Match => "match",
            Keyword::While => "while",
            Keyword::For => "for",
            Keyword::In => "in",
            Keyword::Break => "break",
            Keyword::Continue => "continue",
            Keyword::Loop => "loop",
            Keyword::Enum => "enum",
            Keyword::Type => "type",
            Keyword::Struct => "struct",
            Keyword::Actor => "actor",
            Keyword::State => "state",
            Keyword::On => "on",
            Keyword::Spawn => "spawn",
            Keyword::Reply => "reply",
            Keyword::Yield => "yield",
            Keyword::Await => "await",
            Keyword::Shared => "shared",
            Keyword::Transact => "transact",
            Keyword::Retry => "retry",
            Keyword::Import => "import",
            Keyword::Lazy => "lazy",
            Keyword::As => "as",
            Keyword::Raise => "raise",
            Keyword::Try => "try",
            Keyword::Rescue => "rescue",
            Keyword::Ensure => "ensure",
            Keyword::Return => "return",
            Keyword::True => "true",
            Keyword::False => "false",
            Keyword::Nil => "nil",
            Keyword::And => "and",
            Keyword::Or => "or",
            Keyword::Not => "not",
        }
    }
}

impl TokenKind {
    /// A short human-readable name used in error messages ("expected ...").
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Int(_) => "integer".into(),
            TokenKind::Decimal(_) => "decimal".into(),
            TokenKind::Str(_) => "string".into(),
            TokenKind::Ident(s) => format!("identifier `{}`", s),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::LParen => "`(`".into(),
            TokenKind::RParen => "`)`".into(),
            TokenKind::LBrace => "`{`".into(),
            TokenKind::RBrace => "`}`".into(),
            TokenKind::LBracket => "`[`".into(),
            TokenKind::RBracket => "`]`".into(),
            TokenKind::HashBrace => "`#{`".into(),
            TokenKind::Comma => "`,`".into(),
            TokenKind::Colon => "`:`".into(),
            TokenKind::DoubleColon => "`::`".into(),
            TokenKind::Semicolon => "`;`".into(),
            TokenKind::Dot => "`.`".into(),
            TokenKind::DotDot => "`..`".into(),
            TokenKind::Arrow => "`->`".into(),
            TokenKind::FatArrow => "`=>`".into(),
            TokenKind::Pipe => "`|>`".into(),
            TokenKind::Bang => "`!`".into(),
            TokenKind::Question => "`?`".into(),
            TokenKind::Assign => "`=`".into(),
            TokenKind::Eq => "`==`".into(),
            TokenKind::Neq => "`!=`".into(),
            TokenKind::Lt => "`<`".into(),
            TokenKind::Gt => "`>`".into(),
            TokenKind::Lte => "`<=`".into(),
            TokenKind::Gte => "`>=`".into(),
            TokenKind::Plus => "`+`".into(),
            TokenKind::Minus => "`-`".into(),
            TokenKind::Star => "`*`".into(),
            TokenKind::Slash => "`/`".into(),
            TokenKind::Percent => "`%`".into(),
            TokenKind::PlusAssign => "`+=`".into(),
            TokenKind::MinusAssign => "`-=`".into(),
            TokenKind::StarAssign => "`*=`".into(),
            TokenKind::SlashAssign => "`/=`".into(),
            TokenKind::PercentAssign => "`%=`".into(),
            TokenKind::And => "`&&`".into(),
            TokenKind::Or => "`||`".into(),
            TokenKind::Pipe2 => "`|`".into(),
            TokenKind::Underscore => "`_`".into(),
            TokenKind::Eof => "end of file".into(),
        }
    }
}
