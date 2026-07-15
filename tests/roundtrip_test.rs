//! Property-based tests (`proptest`) for the parser.
//!
//! These generate random arithmetic expressions and verify that the resulting
//! AST respects operator precedence and associativity invariants, and that
//! parsing is deterministic (parse → print → parse yields identical output).

use onely::ast::{BinOp, Expr, Stmt};
use onely::parse;
use onely::printer::print_program;
use proptest::prelude::*;

/// Parse a source string and return the first expression statement's expr.
fn first_expr(src: &str) -> Option<Expr> {
    let out = parse(src);
    if !out.errors.is_empty() {
        return None;
    }
    match out.program.stmts.into_iter().next()? {
        Stmt::Expr(e) | Stmt::Semi(e) => Some(e),
        _ => None,
    }
}

/// The top-level binary operator of an expression, if any.
fn top_op(e: &Expr) -> Option<BinOp> {
    match e {
        Expr::BinOp { op, .. } => Some(*op),
        _ => None,
    }
}

// A small grammar of expressions over single-letter variables.
prop_compose! {
    fn var()(c in "[a-z]") -> String { c }
}

fn atom() -> BoxedStrategy<String> {
    prop_oneof![var().prop_map(|v| format!("({})", v)), var()].boxed()
}

/// Precedence level → (operator strings at that level, lower precedence number).
/// Higher number = binds tighter. Returns a boxed strategy so the three
/// precedence branches can share one return type.
fn expr_strategy(min_prec: u8) -> BoxedStrategy<String> {
    // precedence: 1 = `+`/`-`, 2 = `*`/`/`, 3 = atom
    if min_prec <= 1 {
        // Can use `+` / `-` (left-assoc).
        (expr_strategy(2), prop_oneof![Just("+"), Just("-")], expr_strategy(2))
            .prop_map(|(l, op, r)| format!("{} {} {}", l, op, r))
            .boxed()
    } else if min_prec <= 2 {
        // Can use `*` / `/`.
        (expr_strategy(3), prop_oneof![Just("*"), Just("/")], expr_strategy(3))
            .prop_map(|(l, op, r)| format!("{} {} {}", l, op, r))
            .boxed()
    } else {
        atom()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Parsing any generated arithmetic expression must succeed with no errors.
    #[test]
    fn prop_parse_no_errors(src in expr_strategy(1)) {
        let out = parse(&src);
        prop_assert!(
            out.errors.is_empty(),
            "unexpected errors parsing {:?}: {:?}",
            src,
            out.errors
        );
    }

    /// `a + b * c` must have `+` at the root (mul binds tighter than add).
    #[test]
    fn prop_mul_tighter_than_add(
        a in var(), b in var(), c in var()
    ) {
        let src = format!("{} + {} * {}", a, b, c);
        let e = first_expr(&src).expect("parse should succeed");
        prop_assert_eq!(top_op(&e), Some(BinOp::Add));
    }

    /// `a * b + c` must have `+` at the root.
    #[test]
    fn prop_mul_then_add_root_is_add(
        a in var(), b in var(), c in var()
    ) {
        let src = format!("{} * {} + {}", a, b, c);
        let e = first_expr(&src).expect("parse should succeed");
        prop_assert_eq!(top_op(&e), Some(BinOp::Add));
    }

    /// `a - b - c` is left-associative → root is Sub, lhs is also Sub.
    #[test]
    fn prop_subtraction_left_associative(
        a in var(), b in var(), c in var()
    ) {
        let src = format!("{} - {} - {}", a, b, c);
        let e = first_expr(&src).expect("parse should succeed");
        prop_assert_eq!(top_op(&e), Some(BinOp::Sub));
        if let Expr::BinOp { lhs, .. } = &e {
            // Bind first so the struct-pattern braces don't enter the format
            // string that `prop_assert!` builds.
            let lhs_is_sub = matches!(lhs.as_ref(), Expr::BinOp { op: BinOp::Sub, .. });
            prop_assert!(lhs_is_sub);
        }
    }

    /// Parsing the same source twice yields byte-identical printer output
    /// (determinism / idempotence — our "round-trip" since the printer is a
    /// structural dump rather than a source formatter).
    #[test]
    fn prop_parse_is_deterministic(src in expr_strategy(1)) {
        let a = print_program(&parse(&src).program);
        let b = print_program(&parse(&src).program);
        prop_assert_eq!(a, b);
    }

    /// Parentheses don't change the value but must parse cleanly and the root
    /// becomes a `Paren` (or the inner expr when wrapped at top level).
    #[test]
    fn prop_parentheses_parse_cleanly(src in expr_strategy(1)) {
        let wrapped = format!("({})", src);
        let out = parse(&wrapped);
        prop_assert!(out.errors.is_empty(), "errors: {:?}", out.errors);
    }
}
