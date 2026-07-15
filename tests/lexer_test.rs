//! Lexer unit tests: token kinds, span tracking, numeric edge cases,
//! string interpolation, comments, and error recovery (no panics).

use bigdecimal::BigDecimal;
use num_bigint::BigInt;
use onely::ast::Pos;
use onely::lexer::{tokenize, Keyword, StrPart, TokenKind};

/// Collect just the non-EOF token kinds from a source string.
fn kinds(src: &str) -> Vec<TokenKind> {
    let out = tokenize(src);
    out.tokens
        .into_iter()
        .filter(|t| !matches!(t.kind, TokenKind::Eof))
        .map(|t| t.kind)
        .collect()
}

fn errors(src: &str) -> usize {
    tokenize(src).errors.len()
}

// --------------------------------------------------------------------------
// literals
// --------------------------------------------------------------------------

#[test]
fn lex_integer_literal() {
    let k = kinds("42");
    assert!(matches!(&k[0], TokenKind::Int(n) if *n == BigInt::from(42)));
}

#[test]
fn lex_big_integer_preserved() {
    // 100-digit number must round-trip exactly (no overflow / truncation).
    let s = "1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890";
    let k = kinds(s);
    let n: BigInt = s.parse().unwrap();
    assert!(matches!(&k[0], TokenKind::Int(got) if *got == n));
}

#[test]
fn lex_numeric_separators() {
    let k = kinds("1_000_000");
    assert!(matches!(&k[0], TokenKind::Int(n) if *n == BigInt::from(1_000_000)));
}

#[test]
fn lex_decimal_literal() {
    let k = kinds("3.14");
    assert!(matches!(&k[0], TokenKind::Decimal(d) if *d == BigDecimal::from(314) / BigDecimal::from(100)));
}

#[test]
fn lex_decimal_with_separators() {
    let k = kinds("1_000.50_00");
    assert!(matches!(&k[0], TokenKind::Decimal(d) if *d == BigDecimal::from(10005000) / BigDecimal::from(10000)));
}

#[test]
fn lex_integer_exponent_promotes_to_decimal() {
    // `5e2` is an integer literal with exponent → promoted to decimal 500.
    let k = kinds("5e2");
    assert!(matches!(&k[0], TokenKind::Decimal(_)), "expected decimal, got {:?}", k[0]);
    assert!(matches!(&k[0], TokenKind::Decimal(d) if *d == BigDecimal::from(500)));
}

#[test]
fn lex_decimal_exponent() {
    let k = kinds("1.5e3");
    assert!(matches!(&k[0], TokenKind::Decimal(d) if *d == BigDecimal::from(1500)));
}

#[test]
fn lex_zero() {
    let k = kinds("0");
    assert!(matches!(&k[0], TokenKind::Int(n) if *n == BigInt::from(0)));
}

// --------------------------------------------------------------------------
// identifiers & keywords
// --------------------------------------------------------------------------

#[test]
fn lex_identifier() {
    let k = kinds("foo_bar");
    assert!(matches!(&k[0], TokenKind::Ident(ref s) if s == "foo_bar"));
}

#[test]
fn lex_all_keywords() {
    let src = "let fn if else match enum type struct actor state on spawn reply shared transact import lazy as raise try rescue ensure return true false nil and or not";
    let k = kinds(src);
    let expected = [
        Keyword::Let, Keyword::Fn, Keyword::If, Keyword::Else, Keyword::Match,
        Keyword::Enum, Keyword::Type, Keyword::Struct, Keyword::Actor,
        Keyword::State, Keyword::On, Keyword::Spawn, Keyword::Reply,
        Keyword::Shared, Keyword::Transact, Keyword::Import, Keyword::Lazy,
        Keyword::As, Keyword::Raise, Keyword::Try, Keyword::Rescue,
        Keyword::Ensure, Keyword::Return, Keyword::True, Keyword::False,
        Keyword::Nil, Keyword::And, Keyword::Or, Keyword::Not,
    ];
    assert_eq!(k.len(), expected.len(), "keyword count mismatch");
    for (i, kw) in expected.iter().enumerate() {
        assert!(
            matches!(&k[i], TokenKind::Keyword(got) if got == kw),
            "token {}: expected {:?}, got {:?}", i, kw, k[i]
        );
    }
}

#[test]
fn lex_underscore_is_wildcard() {
    let k = kinds("_");
    assert!(matches!(&k[0], TokenKind::Underscore));
}

// --------------------------------------------------------------------------
// punctuation
// --------------------------------------------------------------------------

#[test]
fn lex_multichar_punctuation() {
    let k = kinds("-> => |> .. :: == != <= >= && || #{");
    let expected = [
        TokenKind::Arrow, TokenKind::FatArrow, TokenKind::Pipe, TokenKind::DotDot,
        TokenKind::DoubleColon, TokenKind::Eq, TokenKind::Neq, TokenKind::Lte,
        TokenKind::Gte, TokenKind::And, TokenKind::Or, TokenKind::HashBrace,
    ];
    assert_eq!(k.len(), expected.len());
    for (i, _) in expected.iter().enumerate() {
        assert!(
            std::mem::discriminant(&k[i]) == std::mem::discriminant(&expected[i]),
            "token {}: expected {:?}, got {:?}", i, expected[i], k[i]
        );
    }
}

#[test]
fn lex_single_char_punctuation() {
    let k = kinds("( ) { } [ ] , : ; . + - * / % < > = ! ?");
    assert_eq!(k.len(), 20);
}

// --------------------------------------------------------------------------
// span tracking
// --------------------------------------------------------------------------

#[test]
fn lex_span_positions() {
    let out = tokenize("let x = 1");
    // `let` should start at line 1, col 1.
    let let_tok = &out.tokens[0];
    assert_eq!(let_tok.span.start, Pos::new(0, 1, 1));
    assert_eq!(let_tok.span.end, Pos::new(3, 1, 4));
    // `1` literal at col 9.
    let one = &out.tokens[3];
    assert_eq!(one.span.start.col, 9);
}

#[test]
fn lex_span_tracks_newlines() {
    let out = tokenize("a\nb");
    // `b` is on line 2, col 1.
    let b = &out.tokens[1];
    assert_eq!(b.span.start.line, 2);
    assert_eq!(b.span.start.col, 1);
}

// --------------------------------------------------------------------------
// strings & interpolation
// --------------------------------------------------------------------------

#[test]
fn lex_plain_string() {
    let k = kinds("\"hello\"");
    match &k[0] {
        TokenKind::Str(parts) => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], StrPart::Literal(ref s) if s == "hello"));
        }
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn lex_string_with_interpolation() {
    // `"x={y}"` → Literal("x=") + Expr("y")
    let k = kinds("\"x={y}\"");
    match &k[0] {
        TokenKind::Str(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], StrPart::Literal(ref s) if s == "x="));
            assert!(matches!(&parts[1], StrPart::Expr(ref src, _) if src == "y"));
        }
        other => panic!("expected Str, got {:?}", other),
    }
}

#[test]
fn lex_triple_quoted_multiline_string() {
    let src = "\"\"\"\nline one\nline two\n\"\"\"";
    let k = kinds(src);
    match &k[0] {
        TokenKind::Str(parts) => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], StrPart::Literal(ref s) if s.contains("line one") && s.contains("line two")));
        }
        other => panic!("expected Str, got {:?}", other),
    }
}

// --------------------------------------------------------------------------
// comments
// --------------------------------------------------------------------------

#[test]
fn lex_line_comment() {
    let k = kinds("// this is a comment\n42");
    assert_eq!(k.len(), 1);
    assert!(matches!(&k[0], TokenKind::Int(_)));
}

#[test]
fn lex_nested_block_comment() {
    // Block comments nest: the outer `/* ... */` contains an inner `/* */`.
    let k = kinds("/* outer /* inner */ still outer */ 42");
    assert_eq!(k.len(), 1);
    assert!(matches!(&k[0], TokenKind::Int(n) if *n == BigInt::from(42)));
}

// --------------------------------------------------------------------------
// error recovery (never panics)
// --------------------------------------------------------------------------

#[test]
fn lex_unterminated_block_comment_is_an_error() {
    let e = errors("/* never closed");
    assert_eq!(e, 1);
    // Lexer still produces an EOF token.
    let out = tokenize("/* never closed");
    assert!(matches!(out.tokens.last().unwrap().kind, TokenKind::Eof));
}

#[test]
fn lex_unterminated_string_is_an_error() {
    assert_eq!(errors("\"no close quote"), 1);
}

#[test]
fn lex_bare_ampersand_is_an_error() {
    // A single `&` is not a valid token (must be `&&`).
    assert!(errors("a & b") >= 1);
}

#[test]
fn lex_bare_hash_is_an_error() {
    // A bare `#` is invalid (sets use `#{ ... }`).
    assert!(errors("#") >= 1);
}

#[test]
fn lex_unexpected_char_does_not_panic() {
    // Control characters and other oddities should produce errors, not panics.
    let _ = tokenize("@\x01\x02");
    // We only assert it didn't panic; the exact error count is not load-bearing.
}

#[test]
fn lex_error_recovery_continues_after_bad_char() {
    // After a bad `@`, the lexer should still find `42`.
    let out = tokenize("@ 42");
    let ints: Vec<_> = out
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Int(_)))
        .collect();
    assert_eq!(ints.len(), 1);
}

#[test]
fn lex_eof_token_always_present() {
    let out = tokenize("");
    assert_eq!(out.tokens.len(), 1);
    assert!(matches!(out.tokens[0].kind, TokenKind::Eof));
}

#[test]
fn lex_empty_input_has_no_errors() {
    assert_eq!(errors(""), 0);
}
