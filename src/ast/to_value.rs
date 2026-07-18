//! Convert AST nodes to 1y [`Value`]s (Map / Vec / Str / Int / Bool).
//!
//! Used by the `ast_of(src)` builtin to expose parsed ASTs to 1y programs
//! at runtime — enabling introspection, macro-like code generation, and
//! self-bootstrapping phases 2-5 (an 1y-written compiler can read its own
//! input via `ast_of`).
//!
//! # Encoding
//!
//! Every node becomes a `Map` with a `"type"` field naming the node kind.
//! Sub-expressions are recursive Maps; lists of children are `Vec`s.
//! Spans are omitted (they are implementation details).
//!
//! Literal values are encoded with their native 1y type:
//!   - `Int(n)` → `Value::Int(n)`
//!   - `Str(parts)` → `Value::Str` (only when no interpolation; otherwise
//!     `{"type": "Str", "parts": [...]}`)
//!   - `Bool(b)` → `Value::Bool(b)`
//!   - `nil` → `Value::Nil`

use crate::ast::*;
use crate::value::Value;

/// Helper: build a Map from a Vec of (Str key, Value).
fn map_of(entries: Vec<(&str, Value)>) -> Value {
    let entries: Vec<(Value, Value)> = entries
        .into_iter()
        .map(|(k, v)| (Value::str(k.to_string()), v))
        .collect();
    Value::map(entries)
}

/// Convert a `&str`-like into a Value::Str.
fn str_val(s: &str) -> Value {
    Value::str(s.to_string())
}

/// Encode a [`Program`] as `{"type": "Program", "stmts": [...]}`.
pub fn program_to_value(p: &Program) -> Value {
    map_of(vec![
        ("type", str_val("Program")),
        (
            "stmts",
            Value::vec(p.stmts.iter().map(stmt_to_value).collect()),
        ),
    ])
}

pub fn stmt_to_value(stmt: &Stmt) -> Value {
    match stmt {
        Stmt::Let { name, value, .. } => map_of(vec![
            ("type", str_val("Let")),
            ("name", str_val(name)),
            ("value", expr_to_value(value)),
        ]),
        Stmt::SharedDecl { name, value, .. } => map_of(vec![
            ("type", str_val("SharedDecl")),
            ("name", str_val(name)),
            ("value", expr_to_value(value)),
        ]),
        Stmt::StateDecl { name, value, .. } => map_of(vec![
            ("type", str_val("StateDecl")),
            ("name", str_val(name)),
            ("value", expr_to_value(value)),
        ]),
        Stmt::FuncDef(f) => map_of(vec![
            ("type", str_val("FuncDef")),
            ("name", str_val(&f.name)),
            (
                "params",
                Value::vec(f.params.iter().map(param_to_value).collect()),
            ),
            ("body", expr_to_value(&f.body)),
        ]),
        Stmt::TypeDef(t) => map_of(vec![
            ("type", str_val("TypeDef")),
            ("name", str_val(&t.name)),
            (
                "type_params",
                Value::vec(t.type_params.iter().map(|p| str_val(p)).collect()),
            ),
            (
                "fields",
                Value::vec(
                    t.fields
                        .iter()
                        .map(|(n, ty)| {
                            map_of(vec![
                                ("name", str_val(n)),
                                ("annot", type_annot_to_value(ty)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        Stmt::EnumDef(e) => map_of(vec![
            ("type", str_val("EnumDef")),
            ("name", str_val(&e.name)),
            (
                "type_params",
                Value::vec(e.type_params.iter().map(|p| str_val(p)).collect()),
            ),
            (
                "variants",
                Value::vec(
                    e.variants
                        .iter()
                        .map(|v| {
                            map_of(vec![
                                ("name", str_val(&v.name)),
                                (
                                    "fields",
                                    Value::vec(
                                        v.fields.iter().map(type_annot_to_value).collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        Stmt::ActorDef(a) => map_of(vec![
            ("type", str_val("ActorDef")),
            ("name", str_val(&a.name)),
            (
                "body",
                Value::vec(a.body.iter().map(stmt_to_value).collect()),
            ),
        ]),
        Stmt::Import(i) => map_of(vec![
            ("type", str_val("Import")),
            ("path", str_val(&i.path)),
            (
                "alias",
                i.alias.as_ref().map(|x| str_val(x)).unwrap_or(Value::Nil),
            ),
            ("lazy", Value::Bool(i.lazy)),
        ]),
        Stmt::OnClause(o) => map_of(vec![
            ("type", str_val("OnClause")),
            ("name", str_val(&o.name)),
            (
                "params",
                Value::vec(o.params.iter().map(param_to_value).collect()),
            ),
            ("body", expr_to_value(&o.body)),
        ]),
        Stmt::Expr(e) => map_of(vec![
            ("type", str_val("Expr")),
            ("expr", expr_to_value(e)),
        ]),
        Stmt::Semi(e) => map_of(vec![
            ("type", str_val("Semi")),
            ("expr", expr_to_value(e)),
        ]),
    }
}

fn param_to_value(p: &Param) -> Value {
    map_of(vec![
        ("name", str_val(&p.name)),
        (
            "annot",
            p.type_annot
                .as_ref()
                .map(type_annot_to_value)
                .unwrap_or(Value::Nil),
        ),
    ])
}

fn type_annot_to_value(t: &TypeAnnot) -> Value {
    match t {
        TypeAnnot::Name { name, .. } => map_of(vec![
            ("type", str_val("TypeName")),
            ("name", str_val(name)),
        ]),
        TypeAnnot::Generic { name, args, .. } => map_of(vec![
            ("type", str_val("TypeGeneric")),
            ("name", str_val(name)),
            (
                "args",
                Value::vec(args.iter().map(type_annot_to_value).collect()),
            ),
        ]),
        TypeAnnot::Union { variants, .. } => map_of(vec![
            ("type", str_val("TypeUnion")),
            (
                "variants",
                Value::vec(variants.iter().map(type_annot_to_value).collect()),
            ),
        ]),
        TypeAnnot::Fn { params, ret, .. } => map_of(vec![
            ("type", str_val("TypeFn")),
            (
                "params",
                Value::vec(params.iter().map(type_annot_to_value).collect()),
            ),
            ("ret", type_annot_to_value(ret)),
        ]),
    }
}

fn str_part_to_value(p: &StrPart) -> Value {
    match p {
        StrPart::Literal(text) => map_of(vec![
            ("type", str_val("Literal")),
            ("text", str_val(text)),
        ]),
        StrPart::Expr(e) => map_of(vec![
            ("type", str_val("Expr")),
            ("expr", expr_to_value(e)),
        ]),
    }
}

pub fn expr_to_value(e: &Expr) -> Value {
    match e {
        Expr::Int(n, _) => map_of(vec![
            ("type", str_val("Int")),
            ("value", Value::Int(n.clone())),
        ]),
        Expr::Decimal(d, _) => map_of(vec![
            ("type", str_val("Decimal")),
            ("value", Value::Decimal(d.clone())),
        ]),
        Expr::Bool(b, _) => map_of(vec![
            ("type", str_val("Bool")),
            ("value", Value::Bool(*b)),
        ]),
        Expr::Nil(_) => map_of(vec![("type", str_val("Nil"))]),
        Expr::Ident(name, _) => map_of(vec![
            ("type", str_val("Ident")),
            ("name", str_val(name)),
        ]),
        Expr::Str(lit, _) => {
            // If the literal has a single Literal part, encode as a plain
            // string value (more convenient for consumers). Otherwise
            // expose the parts list.
            if lit.parts.len() == 1 {
                if let StrPart::Literal(text) = &lit.parts[0] {
                    return map_of(vec![
                        ("type", str_val("Str")),
                        ("value", str_val(text)),
                    ]);
                }
            }
            map_of(vec![
                ("type", str_val("Str")),
                (
                    "parts",
                    Value::vec(lit.parts.iter().map(str_part_to_value).collect()),
                ),
            ])
        }
        Expr::VecLit { items, .. } => map_of(vec![
            ("type", str_val("VecLit")),
            (
                "items",
                Value::vec(items.iter().map(expr_to_value).collect()),
            ),
        ]),
        Expr::MapLit { entries, .. } => map_of(vec![
            ("type", str_val("MapLit")),
            (
                "entries",
                Value::vec(
                    entries
                        .iter()
                        .map(|(k, v)| {
                            Value::vec(vec![expr_to_value(k), expr_to_value(v)])
                        })
                        .collect(),
                ),
            ),
        ]),
        Expr::SetLit { items, .. } => map_of(vec![
            ("type", str_val("SetLit")),
            (
                "items",
                Value::vec(items.iter().map(expr_to_value).collect()),
            ),
        ]),
        Expr::BinOp { op, lhs, rhs, .. } => map_of(vec![
            ("type", str_val("BinOp")),
            ("op", str_val(op.as_str())),
            ("lhs", expr_to_value(lhs)),
            ("rhs", expr_to_value(rhs)),
        ]),
        Expr::UnaryOp { op, expr, .. } => map_of(vec![
            ("type", str_val("UnaryOp")),
            (
                "op",
                str_val(match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "not",
                }),
            ),
            ("expr", expr_to_value(expr)),
        ]),
        Expr::Pipe { lhs, rhs, .. } => map_of(vec![
            ("type", str_val("Pipe")),
            ("lhs", expr_to_value(lhs)),
            ("rhs", expr_to_value(rhs)),
        ]),
        Expr::If { cond, then, else_, .. } => {
            let mut fields = vec![
                ("type", str_val("If")),
                ("cond", expr_to_value(cond)),
                ("then", expr_to_value(then)),
            ];
            fields.push((
                "else",
                else_
                    .as_ref()
                    .map(|e| expr_to_value(e))
                    .unwrap_or(Value::Nil),
            ));
            map_of(fields)
        }
        Expr::Match { scrutinee, arms, .. } => map_of(vec![
            ("type", str_val("Match")),
            ("scrutinee", expr_to_value(scrutinee)),
            (
                "arms",
                Value::vec(
                    arms.iter()
                        .map(|a| {
                            map_of(vec![
                                ("pattern", pattern_to_value(&a.pattern)),
                                (
                                    "guard",
                                    a.guard
                                        .as_ref()
                                        .map(|g| expr_to_value(g))
                                        .unwrap_or(Value::Nil),
                                ),
                                ("body", expr_to_value(&a.body)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
        Expr::Lambda {
            params, body, ..
        } => map_of(vec![
            ("type", str_val("Lambda")),
            (
                "params",
                Value::vec(params.iter().map(param_to_value).collect()),
            ),
            ("body", expr_to_value(body)),
        ]),
        Expr::Block { stmts, tail, .. } => map_of(vec![
            ("type", str_val("Block")),
            (
                "stmts",
                Value::vec(stmts.iter().map(stmt_to_value).collect()),
            ),
            (
                "tail",
                tail.as_ref()
                    .map(|t| expr_to_value(t))
                    .unwrap_or(Value::Nil),
            ),
        ]),
        Expr::Paren(inner, _) => map_of(vec![
            ("type", str_val("Paren")),
            ("expr", expr_to_value(inner)),
        ]),
        Expr::Call { callee, args, .. } => map_of(vec![
            ("type", str_val("Call")),
            ("callee", expr_to_value(callee)),
            (
                "args",
                Value::vec(args.iter().map(expr_to_value).collect()),
            ),
        ]),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => map_of(vec![
            ("type", str_val("MethodCall")),
            ("receiver", expr_to_value(receiver)),
            ("method", str_val(method)),
            (
                "args",
                Value::vec(args.iter().map(expr_to_value).collect()),
            ),
        ]),
        Expr::Index { target, index, .. } => map_of(vec![
            ("type", str_val("Index")),
            ("target", expr_to_value(target)),
            ("index", expr_to_value(index)),
        ]),
        Expr::Field { target, name, .. } => map_of(vec![
            ("type", str_val("Field")),
            ("target", expr_to_value(target)),
            ("name", str_val(name)),
        ]),
        Expr::ActorSend { actor, msg, .. } => map_of(vec![
            ("type", str_val("ActorSend")),
            ("actor", expr_to_value(actor)),
            ("msg", expr_to_value(msg)),
        ]),
        Expr::ActorRequest { actor, msg, .. } => map_of(vec![
            ("type", str_val("ActorRequest")),
            ("actor", expr_to_value(actor)),
            ("msg", expr_to_value(msg)),
        ]),
        Expr::Spawn { name, args, .. } => map_of(vec![
            ("type", str_val("Spawn")),
            ("name", str_val(name)),
            (
                "args",
                Value::vec(args.iter().map(expr_to_value).collect()),
            ),
        ]),
        Expr::Raise { expr, .. } => map_of(vec![
            ("type", str_val("Raise")),
            ("expr", expr_to_value(expr)),
        ]),
        Expr::Try {
            body,
            rescues,
            ensure,
            ..
        } => map_of(vec![
            ("type", str_val("Try")),
            ("body", expr_to_value(body)),
            (
                "rescues",
                Value::vec(
                    rescues
                        .iter()
                        .map(|r| {
                            map_of(vec![
                                (
                                    "type_name",
                                    r.type_name
                                        .as_ref()
                                        .map(|t| str_val(t))
                                        .unwrap_or(Value::Nil),
                                ),
                                (
                                    "bind",
                                    r.bind.as_ref().map(|b| str_val(b)).unwrap_or(Value::Nil),
                                ),
                                ("body", expr_to_value(&r.body)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "ensure",
                ensure.as_ref().map(|e| expr_to_value(e)).unwrap_or(Value::Nil),
            ),
        ]),
        Expr::Transact { body, .. } => map_of(vec![
            ("type", str_val("Transact")),
            ("body", expr_to_value(body)),
        ]),
        Expr::Retry { .. } => map_of(vec![("type", str_val("Retry"))]),
        Expr::Return { value, .. } => map_of(vec![
            ("type", str_val("Return")),
            (
                "value",
                value.as_ref().map(|v| expr_to_value(v)).unwrap_or(Value::Nil),
            ),
        ]),
        Expr::Reply { value, .. } => map_of(vec![
            ("type", str_val("Reply")),
            ("value", expr_to_value(value)),
        ]),
        Expr::Yield { .. } => map_of(vec![("type", str_val("Yield"))]),
        Expr::Await { expr, .. } => map_of(vec![
            ("type", str_val("Await")),
            ("expr", expr_to_value(expr)),
        ]),
        Expr::SharedExpr { expr, .. } => map_of(vec![
            ("type", str_val("SharedExpr")),
            ("expr", expr_to_value(expr)),
        ]),
        Expr::Assign { target, value, .. } => map_of(vec![
            ("type", str_val("Assign")),
            ("target", expr_to_value(target)),
            ("value", expr_to_value(value)),
        ]),
        Expr::CompoundAssign {
            op,
            target,
            value,
            ..
        } => map_of(vec![
            ("type", str_val("CompoundAssign")),
            ("op", str_val(op.as_str())),
            ("target", expr_to_value(target)),
            ("value", expr_to_value(value)),
        ]),
        Expr::While { cond, body, .. } => map_of(vec![
            ("type", str_val("While")),
            ("cond", expr_to_value(cond)),
            ("body", expr_to_value(body)),
        ]),
        Expr::For { var, iter, body, .. } => map_of(vec![
            ("type", str_val("For")),
            ("var", str_val(var)),
            ("iter", expr_to_value(iter)),
            ("body", expr_to_value(body)),
        ]),
        Expr::Loop { body, .. } => map_of(vec![
            ("type", str_val("Loop")),
            ("body", expr_to_value(body)),
        ]),
        Expr::Break { value, .. } => map_of(vec![
            ("type", str_val("Break")),
            (
                "value",
                value.as_ref().map(|v| expr_to_value(v)).unwrap_or(Value::Nil),
            ),
        ]),
        Expr::Continue { .. } => map_of(vec![("type", str_val("Continue"))]),
    }
}

fn pattern_to_value(p: &Pattern) -> Value {
    match p {
        Pattern::Wildcard(_) => map_of(vec![("type", str_val("Wildcard"))]),
        Pattern::Lit(lit, _) => match lit {
            LitPattern::Int(n) => map_of(vec![
                ("type", str_val("Lit")),
                ("lit_type", str_val("Int")),
                ("value", Value::Int(n.clone())),
            ]),
            LitPattern::Decimal(d) => map_of(vec![
                ("type", str_val("Lit")),
                ("lit_type", str_val("Decimal")),
                ("value", Value::Decimal(d.clone())),
            ]),
            LitPattern::Str(parts) => map_of(vec![
                ("type", str_val("Lit")),
                ("lit_type", str_val("Str")),
                (
                    "parts",
                    Value::vec(parts.iter().map(str_part_to_value).collect()),
                ),
            ]),
            LitPattern::Bool(b) => map_of(vec![
                ("type", str_val("Lit")),
                ("lit_type", str_val("Bool")),
                ("value", Value::Bool(*b)),
            ]),
            LitPattern::Nil => map_of(vec![
                ("type", str_val("Lit")),
                ("lit_type", str_val("Nil")),
            ]),
        },
        Pattern::Bind(name, _) => map_of(vec![
            ("type", str_val("Bind")),
            ("name", str_val(name)),
        ]),
        Pattern::Variant { name, args, .. } => map_of(vec![
            ("type", str_val("Variant")),
            ("name", str_val(name)),
            (
                "args",
                Value::vec(args.iter().map(pattern_to_value).collect()),
            ),
        ]),
        Pattern::Struct { fields, rest, .. } => map_of(vec![
            ("type", str_val("Struct")),
            (
                "fields",
                Value::vec(
                    fields
                        .iter()
                        .map(|(n, p)| {
                            map_of(vec![
                                ("name", str_val(n)),
                                ("pattern", pattern_to_value(p)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("rest", Value::Bool(*rest)),
        ]),
        Pattern::Vec { pats, rest, .. } => map_of(vec![
            ("type", str_val("Vec")),
            (
                "pats",
                Value::vec(pats.iter().map(pattern_to_value).collect()),
            ),
            ("rest", Value::Bool(*rest)),
        ]),
        Pattern::Or(pats, _) => map_of(vec![
            ("type", str_val("Or")),
            (
                "pats",
                Value::vec(pats.iter().map(pattern_to_value).collect()),
            ),
        ]),
        Pattern::Guard(pat, guard, _) => map_of(vec![
            ("type", str_val("Guard")),
            ("pattern", pattern_to_value(pat)),
            ("guard", expr_to_value(guard)),
        ]),
    }
}
