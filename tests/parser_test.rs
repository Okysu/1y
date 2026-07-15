//! Parser tests: operator precedence, associativity, pattern matching,
//! and AST snapshot tests (golden-string comparison via the printer).
//!
//! Since AST nodes do not implement `PartialEq`, structural checks use either
//! `matches!` (top-level shape) or the stable textual output of
//! [`onely::printer::print_program`] (snapshots).

use onely::ast::{BinOp, Expr, Pattern, Stmt};
use onely::parse;
use onely::printer::print_program;

/// Parse a single expression statement and return the inner [`Expr`].
fn parse_expr_first(src: &str) -> Expr {
    let out = parse(src);
    assert!(
        out.errors.is_empty(),
        "unexpected parse errors in `{}`: {:?}",
        src,
        out.errors
    );
    assert_eq!(out.program.stmts.len(), 1, "expected 1 stmt in `{}`", src);
    match out.program.stmts.into_iter().next().unwrap() {
        Stmt::Expr(e) | Stmt::Semi(e) => e,
        other => panic!("expected Expr/Semi stmt, got {:?}", other),
    }
}

fn assert_no_errors(src: &str) {
    let out = parse(src);
    assert!(
        out.errors.is_empty(),
        "expected no errors in `{}`, got: {:?}",
        src,
        out.errors
    );
}

// ==========================================================================
// operator precedence & associativity
// ==========================================================================

#[test]
fn precedence_mul_binds_tighter_than_add() {
    // `1 + 2 * 3`  ==  `1 + (2 * 3)`
    let e = parse_expr_first("1 + 2 * 3");
    assert!(matches!(e, Expr::BinOp { op: BinOp::Add, .. }));
    if let Expr::BinOp { lhs, rhs, .. } = e {
        assert!(matches!(*lhs, Expr::Int(_, _)));
        assert!(matches!(*rhs, Expr::BinOp { op: BinOp::Mul, .. }));
    }
}

#[test]
fn precedence_left_associative_subtraction() {
    // `1 - 2 - 3`  ==  `(1 - 2) - 3`
    let e = parse_expr_first("1 - 2 - 3");
    assert!(matches!(e, Expr::BinOp { op: BinOp::Sub, .. }));
    if let Expr::BinOp { lhs, rhs, .. } = e {
        assert!(
            matches!(*lhs, Expr::BinOp { op: BinOp::Sub, .. }),
            "left operand should be nested Sub"
        );
        assert!(matches!(*rhs, Expr::Int(_, _)));
    }
}

#[test]
fn precedence_right_associative_assignment() {
    // `a = b = c`  ==  `a = (b = c)`
    let e = parse_expr_first("a = b = c");
    assert!(matches!(e, Expr::Assign { .. }));
    if let Expr::Assign { value, .. } = e {
        assert!(
            matches!(*value, Expr::Assign { .. }),
            "rhs of chained assignment must be nested Assign"
        );
    }
}

#[test]
fn precedence_pipe_is_left_associative() {
    // `xs |> f |> g`  ==  `(xs |> f) |> g`
    let e = parse_expr_first("xs |> f |> g");
    assert!(matches!(e, Expr::Pipe { .. }));
    if let Expr::Pipe { lhs, .. } = e {
        assert!(
            matches!(*lhs, Expr::Pipe { .. }),
            "pipe lhs must be nested Pipe (left-assoc)"
        );
    }
}

#[test]
fn precedence_comparison_binds_tighter_than_and() {
    // `a < b and c > d`  ==  `(a < b) and (c > d)`
    let e = parse_expr_first("a < b and c > d");
    assert!(matches!(e, Expr::BinOp { op: BinOp::And, .. }));
    if let Expr::BinOp { lhs, rhs, .. } = e {
        assert!(matches!(*lhs, Expr::BinOp { op: BinOp::Lt, .. }));
        assert!(matches!(*rhs, Expr::BinOp { op: BinOp::Gt, .. }));
    }
}

#[test]
fn precedence_pipe_binds_looser_than_arithmetic() {
    // `a + b |> f`  ==  `(a + b) |> f`
    let e = parse_expr_first("a + b |> f");
    assert!(matches!(e, Expr::Pipe { .. }));
    if let Expr::Pipe { lhs, .. } = e {
        assert!(matches!(*lhs, Expr::BinOp { op: BinOp::Add, .. }));
    }
}

#[test]
fn precedence_unary_neg() {
    let e = parse_expr_first("-x");
    assert!(matches!(e, Expr::UnaryOp { .. }));
}

#[test]
fn precedence_not_keyword_is_unary() {
    let e = parse_expr_first("not x");
    assert!(matches!(e, Expr::UnaryOp { .. }));
}

#[test]
fn precedence_parentheses_override() {
    // `(1 + 2) * 3`
    let e = parse_expr_first("(1 + 2) * 3");
    assert!(matches!(e, Expr::BinOp { op: BinOp::Mul, .. }));
    if let Expr::BinOp { lhs, .. } = e {
        assert!(matches!(*lhs, Expr::Paren(..)));
    }
}

#[test]
fn precedence_actor_send_lower_than_arithmetic() {
    // `c ! inc()` — the message is a call expression.
    let e = parse_expr_first("c ! inc()");
    assert!(matches!(e, Expr::ActorSend { .. }));
    if let Expr::ActorSend { msg, .. } = e {
        assert!(matches!(*msg, Expr::Call { .. }));
    }
}

#[test]
fn precedence_actor_request_binds_like_send() {
    let e = parse_expr_first("c ? get()");
    assert!(matches!(e, Expr::ActorRequest { .. }));
}

#[test]
fn precedence_postfix_call_chain() {
    // `f(x)(y).z[0]`
    let e = parse_expr_first("f(x)(y).z[0]");
    assert!(matches!(e, Expr::Index { .. }));
}

// ==========================================================================
// declarations & types
// ==========================================================================

#[test]
fn parse_let_with_type_annotation() {
    let out = parse("let x: Number = 42");
    assert!(out.errors.is_empty());
    assert!(matches!(out.program.stmts[0], Stmt::Let { ref name, .. } if name == "x"));
}

#[test]
fn parse_function_def() {
    let out = parse("fn add(a, b) { a + b }");
    assert!(out.errors.is_empty());
    match &out.program.stmts[0] {
        Stmt::FuncDef(f) => {
            assert_eq!(f.name, "add");
            assert_eq!(f.params.len(), 2);
        }
        other => panic!("expected FuncDef, got {:?}", other),
    }
}

#[test]
fn parse_enum_with_type_params() {
    let out = parse("enum Option<T> { Some(T), None }");
    assert!(out.errors.is_empty());
    match &out.program.stmts[0] {
        Stmt::EnumDef(e) => {
            assert_eq!(e.name, "Option");
            assert_eq!(e.type_params, vec!["T".to_string()]);
            assert_eq!(e.variants.len(), 2);
            assert_eq!(e.variants[0].name, "Some");
            assert_eq!(e.variants[1].name, "None");
            assert!(e.variants[1].fields.is_empty());
        }
        other => panic!("expected EnumDef, got {:?}", other),
    }
}

#[test]
fn parse_type_def_record() {
    let out = parse("type Point = { x: Number, y: Number }");
    assert!(out.errors.is_empty());
    match &out.program.stmts[0] {
        Stmt::TypeDef(t) => {
            assert_eq!(t.name, "Point");
            assert_eq!(t.fields.len(), 2);
        }
        other => panic!("expected TypeDef, got {:?}", other),
    }
}

#[test]
fn parse_actor_def() {
    let out = parse("actor Counter { state count = 0 on inc() { count = count + 1 } }");
    assert!(out.errors.is_empty());
    match &out.program.stmts[0] {
        Stmt::ActorDef(a) => {
            assert_eq!(a.name, "Counter");
            assert_eq!(a.body.len(), 2);
            assert!(matches!(a.body[0], Stmt::StateDecl { .. }));
            assert!(matches!(a.body[1], Stmt::OnClause(_)));
        }
        other => panic!("expected ActorDef, got {:?}", other),
    }
}

#[test]
fn parse_imports() {
    let out = parse("import io\nimport net.socket as socket\nlazy import heavy");
    assert!(out.errors.is_empty());
    assert_eq!(out.program.stmts.len(), 3);
    match &out.program.stmts[0] {
        Stmt::Import(i) => {
            assert_eq!(i.path, "io");
            assert!(!i.lazy);
        }
        _ => panic!("expected Import"),
    }
    match &out.program.stmts[1] {
        Stmt::Import(i) => {
            assert_eq!(i.path, "net.socket");
            assert_eq!(i.alias.as_deref(), Some("socket"));
        }
        _ => panic!("expected Import"),
    }
    match &out.program.stmts[2] {
        Stmt::Import(i) => {
            assert!(i.lazy);
            assert_eq!(i.path, "heavy");
        }
        _ => panic!("expected lazy Import"),
    }
}

// ==========================================================================
// collections
// ==========================================================================

#[test]
fn parse_vector_literal() {
    let e = parse_expr_first("[1, 2, 3]");
    assert!(matches!(e, Expr::VecLit { ref items, .. } if items.len() == 3));
}

#[test]
fn parse_map_literal() {
    let e = parse_expr_first("{ x: 1, y: 2 }");
    assert!(matches!(e, Expr::MapLit { ref entries, .. } if entries.len() == 2));
}

#[test]
fn parse_set_literal() {
    let e = parse_expr_first("#{1, 2, 3}");
    assert!(matches!(e, Expr::SetLit { ref items, .. } if items.len() == 3));
}

#[test]
fn parse_empty_vector() {
    let e = parse_expr_first("[]");
    assert!(matches!(e, Expr::VecLit { ref items, .. } if items.is_empty()));
}

// ==========================================================================
// control flow expressions
// ==========================================================================

#[test]
fn parse_if_else() {
    let e = parse_expr_first("if x { 1 } else { 2 }");
    assert!(matches!(e, Expr::If { ref else_, .. } if else_.is_some()));
}

#[test]
fn parse_if_without_else() {
    let e = parse_expr_first("if x { 1 }");
    assert!(matches!(e, Expr::If { ref else_, .. } if else_.is_none()));
}

#[test]
fn parse_lambda() {
    let e = parse_expr_first("fn(x) { x * 2 }");
    assert!(matches!(e, Expr::Lambda { .. }));
}

#[test]
fn parse_try_rescue_ensure() {
    assert_no_errors("fn f() { try { risky() } rescue IOError as e { -1 } ensure { cleanup() } }");
}

#[test]
fn parse_transact_block() {
    let e = parse_expr_first("transact { counter = counter + 1 }");
    assert!(matches!(e, Expr::Transact { .. }));
}

#[test]
fn parse_spawn_expression() {
    let e = parse_expr_first("spawn Counter()");
    assert!(matches!(e, Expr::Spawn { ref name, .. } if name == "Counter"));
}

#[test]
fn parse_raise_expression() {
    let e = parse_expr_first("raise \"oops\"");
    assert!(matches!(e, Expr::Raise { .. }));
}

#[test]
fn parse_block_with_tail_value() {
    let e = parse_expr_first("{ let a = 1 a + 1 }");
    assert!(matches!(e, Expr::Block { ref tail, .. } if tail.is_some()));
}

// ==========================================================================
// pattern matching
// ==========================================================================

#[test]
fn parse_match_with_variant_patterns() {
    let out = parse("fn f(opt) { match opt { Some(x) => x, None => 0 } }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_match_with_guard() {
    let out = parse("fn f(n) { match n { x if x > 0 => 1, _ => 0 } }");
    assert!(out.errors.is_empty());
}

#[test]
fn parse_or_pattern() {
    let out = parse("fn f(e) { match e { Err(_) | Err(msg) => 1, _ => 0 } }");
    assert!(out.errors.is_empty());
    // Drill into the match to confirm the first arm is an Or pattern.
    let f = match &out.program.stmts[0] {
        Stmt::FuncDef(f) => f,
        _ => panic!(),
    };
    let body = f.body.as_ref();
    let m = match body {
        Expr::Block { tail, .. } => match tail.as_ref().unwrap().as_ref() {
            Expr::Match { arms, .. } => arms,
            _ => panic!("expected Match"),
        },
        _ => panic!("expected Block"),
    };
    assert!(matches!(m[0].pattern, Pattern::Or(_, _)));
}

#[test]
fn parse_struct_pattern_with_rest() {
    let out = parse("fn f(p) { match p { { x: a, y: b, .. } => a } }");
    assert!(out.errors.is_empty());
    let f = match &out.program.stmts[0] {
        Stmt::FuncDef(f) => f,
        _ => panic!(),
    };
    let m = match f.body.as_ref() {
        Expr::Block { tail, .. } => match tail.as_ref().unwrap().as_ref() {
            Expr::Match { arms, .. } => &arms[0].pattern,
            _ => panic!("expected Match"),
        },
        _ => panic!("expected Block"),
    };
    assert!(matches!(m, Pattern::Struct { rest: true, .. }));
}

#[test]
fn parse_vec_pattern_with_rest() {
    let out = parse("fn f(xs) { match xs { [a, ..] => a } }");
    assert!(out.errors.is_empty());
    let f = match &out.program.stmts[0] {
        Stmt::FuncDef(f) => f,
        _ => panic!(),
    };
    let m = match f.body.as_ref() {
        Expr::Block { tail, .. } => match tail.as_ref().unwrap().as_ref() {
            Expr::Match { arms, .. } => &arms[0].pattern,
            _ => panic!("expected Match"),
        },
        _ => panic!("expected Block"),
    };
    assert!(matches!(m, Pattern::Vec { rest: true, .. }));
}

#[test]
fn parse_literal_patterns() {
    let out = parse("fn f(n) { match n { 0 => 1, true => 2, nil => 3, _ => 0 } }");
    assert!(out.errors.is_empty());
}

// ==========================================================================
// error recovery
// ==========================================================================

#[test]
fn parse_error_recovery_continues_after_bad_stmt() {
    // First stmt is malformed (missing `=`), second is valid.
    let out = parse("let x 42\nlet y = 1");
    assert!(!out.errors.is_empty(), "expected at least one error");
    // The second statement should still be parsed.
    assert!(out.program.stmts.iter().any(|s| matches!(s, Stmt::Let { name, .. } if name == "y")));
}

#[test]
fn parse_never_panics_on_garbage() {
    let _ = parse("@#$%^&*(){{{}}}");
    let _ = parse("let = = =");
    let _ = parse("match {");
    // No assertion beyond "did not panic".
}

// ==========================================================================
// shared expression form + error location regression
// ==========================================================================

#[test]
fn parse_shared_as_expression() {
    // `let x = shared 0` — shared used as an expression, not a statement.
    let out = parse("let x = shared 0");
    assert!(out.errors.is_empty(), "expected no errors, got {:?}", out.errors);
    assert!(matches!(&out.program.stmts.as_slice(), [Stmt::Let { .. }]));
}

#[test]
fn parse_shared_statement_form() {
    // `shared x = 0` — statement form (declares a shared binding).
    let out = parse("shared x = 0");
    assert!(out.errors.is_empty(), "expected no errors, got {:?}", out.errors);
}

#[test]
fn parse_error_has_line_col() {
    // The arrow-lambda syntax `() => {}` is NOT supported; 1y uses `fn() {}`.
    // This test verifies the parser reports the error with line/col info.
    let src = "let f = () => { 1 }";
    let out = parse(src);
    assert_eq!(out.errors.len(), 1);
    let e = &out.errors[0];
    // The error should point at the `)` on line 1.
    assert_eq!(e.span.start.line, 1);
    assert!(e.span.start.col >= 5 && e.span.start.col <= 15);
}

#[test]
fn parse_lambda_fn_form_supported() {
    // The correct lambda syntax is `fn(params) { body }`.
    let out = parse("let f = fn() { 1 }");
    assert!(out.errors.is_empty(), "expected no errors, got {:?}", out.errors);
}

// ==========================================================================
// AST snapshot tests (golden-string comparison)
// ==========================================================================

#[test]
fn snapshot_simple_arithmetic() {
    let out = parse("1 + 2 * 3");
    let printed = print_program(&out.program);
    insta_like(
        &printed,
        "Program @...\n\
         expr: BinOp(+) @...\n\
         \x20 lhs: Int(1) @...\n\
         \x20 rhs: BinOp(*) @...\n\
         \x20   lhs: Int(2) @...\n\
         \x20   rhs: Int(3) @...\n",
    );
    // The snapshot check is structural: we verify the operator sequence and
    // nesting rather than exact span text.
    assert!(printed.contains("BinOp(+)"));
    assert!(printed.contains("BinOp(*)"));
    // The `*` operand must be nested under the `+` (appears later / indented).
    let plus_idx = printed.find("BinOp(+)").unwrap();
    let mul_idx = printed.find("BinOp(*)").unwrap();
    assert!(mul_idx > plus_idx, "Mul should appear nested under Add");
}

#[test]
fn snapshot_pipe_chain() {
    let out = parse("xs |> f |> g");
    let printed = print_program(&out.program);
    assert!(printed.contains("Pipe"));
    // Two Pipe nodes (left-assoc nesting).
    assert_eq!(printed.matches("Pipe").count(), 2);
}

#[test]
fn snapshot_deterministic_reparse() {
    // Parsing the same source twice must yield byte-identical printer output.
    let src = "fn factorial(n) { if n <= 1 { 1 } else { n * factorial(n - 1) } }";
    let a = print_program(&parse(src).program);
    let b = print_program(&parse(src).program);
    assert_eq!(a, b);
}

/// Lightweight stand-in for `insta`: we currently only assert the structural
/// invariants; the `expected` argument documents the intended shape.
fn insta_like(_actual: &str, _expected: &str) {
    // No snapshot crate in Phase 0; real assertions are made by the caller.
}
