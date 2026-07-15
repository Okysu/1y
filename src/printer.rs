//! Pretty-printer for the `1y` AST.
//!
//! Produces a stable, indented textual representation used by the `1y parse`
//! command and by snapshot tests. It is *not* a source-code formatter (it
//! deliberately shows structure, e.g. `BinOp(Add, ...)`).

use crate::ast::*;
use bigdecimal::BigDecimal;
use std::fmt::Write;

/// Render a [`Program`] as an indented tree.
pub fn print_program(p: &Program) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Program @{}", p.span);
    let mut w = Writer::new();
    for s in &p.stmts {
        print_stmt(&mut out, &mut w, s);
    }
    out
}

struct Writer {
    indent: usize,
}

impl Writer {
    fn new() -> Self {
        Writer { indent: 0 }
    }
    fn ind(&self) -> String {
        "  ".repeat(self.indent)
    }
    fn push(&mut self) {
        self.indent += 1;
    }
    fn pop(&mut self) {
        if self.indent > 0 {
            self.indent -= 1;
        }
    }
}

fn print_stmt(out: &mut String, w: &mut Writer, s: &Stmt) {
    match s {
        Stmt::Let {
            name,
            type_annot,
            value,
            span,
        } => {
            let _ = writeln!(out, "{}Let `{}` @{}", w.ind(), name, span);
            if let Some(t) = type_annot {
                w.push();
                let _ = writeln!(out, "{}type: {}", w.ind(), print_type(t));
                w.pop();
            }
            print_expr(out, w, "value", value);
        }
        Stmt::SharedDecl {
            name,
            type_annot,
            value,
            span,
        } => {
            let _ = writeln!(out, "{}SharedDecl `{}` @{}", w.ind(), name, span);
            if let Some(t) = type_annot {
                w.push();
                let _ = writeln!(out, "{}type: {}", w.ind(), print_type(t));
                w.pop();
            }
            print_expr(out, w, "init", value);
        }
        Stmt::StateDecl {
            name,
            type_annot,
            value,
            span,
        } => {
            let _ = writeln!(out, "{}StateDecl `{}` @{}", w.ind(), name, span);
            if let Some(t) = type_annot {
                w.push();
                let _ = writeln!(out, "{}type: {}", w.ind(), print_type(t));
                w.pop();
            }
            print_expr(out, w, "init", value);
        }
        Stmt::FuncDef(f) => {
            let _ = writeln!(out, "{}FuncDef `{}` @{}", w.ind(), f.name, f.span);
            w.push();
            for p in &f.params {
                let _ = writeln!(out, "{}param `{}`", w.ind(), p.name);
            }
            if let Some(t) = &f.return_type {
                let _ = writeln!(out, "{}returns: {}", w.ind(), print_type(t));
            }
            w.pop();
            print_expr(out, w, "body", &f.body);
        }
        Stmt::OnClause(o) => {
            let _ = writeln!(out, "{}On `{}` @{}", w.ind(), o.name, o.span);
            w.push();
            for p in &o.params {
                let _ = writeln!(out, "{}param `{}`", w.ind(), p.name);
            }
            if let Some(t) = &o.return_type {
                let _ = writeln!(out, "{}returns: {}", w.ind(), print_type(t));
            }
            w.pop();
            print_expr(out, w, "body", &o.body);
        }
        Stmt::TypeDef(t) => {
            let _ = writeln!(out, "{}TypeDef `{}` @{}", w.ind(), t.name, t.span);
            w.push();
            if !t.type_params.is_empty() {
                let _ = writeln!(out, "{}type_params: {}", w.ind(), t.type_params.join(", "));
            }
            for (n, ty) in &t.fields {
                let _ = writeln!(out, "{}field {}: {}", w.ind(), n, print_type(ty));
            }
            w.pop();
        }
        Stmt::EnumDef(e) => {
            let _ = writeln!(out, "{}EnumDef `{}` @{}", w.ind(), e.name, e.span);
            w.push();
            if !e.type_params.is_empty() {
                let _ = writeln!(out, "{}type_params: {}", w.ind(), e.type_params.join(", "));
            }
            for v in &e.variants {
                let fields = v
                    .fields
                    .iter()
                    .map(print_type)
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "{}{}({})", w.ind(), v.name, fields);
            }
            w.pop();
        }
        Stmt::ActorDef(a) => {
            let _ = writeln!(out, "{}ActorDef `{}` @{}", w.ind(), a.name, a.span);
            w.push();
            for s in &a.body {
                print_stmt(out, w, s);
            }
            w.pop();
        }
        Stmt::Import(i) => {
            let kw = if i.lazy { "lazy import" } else { "import" };
            match &i.alias {
                Some(a) => {
                    let _ = writeln!(out, "{}{} {} as {} @{}", w.ind(), kw, i.path, a, i.span);
                }
                None => {
                    let _ = writeln!(out, "{}{} {} @{}", w.ind(), kw, i.path, i.span);
                }
            }
        }
        Stmt::Expr(e) => {
            print_expr(out, w, "expr", e);
        }
        Stmt::Semi(e) => {
            print_expr(out, w, "expr;", e);
        }
    }
}

fn print_expr(out: &mut String, w: &mut Writer, label: &str, e: &Expr) {
    let _ = writeln!(out, "{}{}: {} @{}", w.ind(), label, expr_head(e), e.span());
    match e {
        Expr::Int(_, _) | Expr::Decimal(_, _) | Expr::Str(_, _) | Expr::Bool(_, _)
        | Expr::Nil(_) | Expr::Ident(_, _) => {}
        Expr::VecLit { items, .. } => {
            w.push();
            for (i, it) in items.iter().enumerate() {
                print_expr(out, w, &format!("[{}]", i), it);
            }
            w.pop();
        }
        Expr::MapLit { entries, .. } => {
            w.push();
            for (i, (k, v)) in entries.iter().enumerate() {
                let _ = writeln!(out, "{}entry {}:", w.ind(), i);
                w.push();
                print_expr(out, w, "key", k);
                print_expr(out, w, "val", v);
                w.pop();
            }
            w.pop();
        }
        Expr::SetLit { items, .. } => {
            w.push();
            for (i, it) in items.iter().enumerate() {
                print_expr(out, w, &format!("set[{}]", i), it);
            }
            w.pop();
        }
        Expr::BinOp { op, lhs, rhs, .. } => {
            w.push();
            print_expr(out, w, "lhs", lhs);
            print_expr(out, w, "rhs", rhs);
            w.pop();
            let _ = op;
        }
        Expr::UnaryOp { expr, .. } => {
            w.push();
            print_expr(out, w, "operand", expr);
            w.pop();
        }
        Expr::Pipe { lhs, rhs, .. } => {
            w.push();
            print_expr(out, w, "lhs", lhs);
            print_expr(out, w, "rhs", rhs);
            w.pop();
        }
        Expr::If { cond, then, else_, .. } => {
            w.push();
            print_expr(out, w, "cond", cond);
            print_expr(out, w, "then", then);
            if let Some(e) = else_ {
                print_expr(out, w, "else", e);
            }
            w.pop();
        }
        Expr::Match { scrutinee, arms, .. } => {
            w.push();
            print_expr(out, w, "scrutinee", scrutinee);
            for (i, arm) in arms.iter().enumerate() {
                let _ = writeln!(out, "{}arm {}:", w.ind(), i);
                w.push();
                let _ = writeln!(out, "{}pattern: {}", w.ind(), print_pattern(&arm.pattern));
                if let Some(g) = &arm.guard {
                    print_expr(out, w, "guard", g);
                }
                print_expr(out, w, "body", &arm.body);
                w.pop();
            }
            w.pop();
        }
        Expr::Lambda { params, body, .. } => {
            w.push();
            for p in params {
                let _ = writeln!(out, "{}param `{}`", w.ind(), p.name);
            }
            w.pop();
            print_expr(out, w, "body", body);
        }
        Expr::Block { stmts, tail, .. } => {
            w.push();
            for s in stmts {
                print_stmt(out, w, s);
            }
            if let Some(t) = tail {
                print_expr(out, w, "tail", t);
            }
            w.pop();
        }
        Expr::Paren(inner, _) => {
            w.push();
            print_expr(out, w, "inner", inner);
            w.pop();
        }
        Expr::Call { callee, args, .. } => {
            w.push();
            print_expr(out, w, "callee", callee);
            for (i, a) in args.iter().enumerate() {
                print_expr(out, w, &format!("arg{}", i), a);
            }
            w.pop();
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            w.push();
            print_expr(out, w, "receiver", receiver);
            let _ = writeln!(out, "{}method: {}", w.ind(), method);
            for (i, a) in args.iter().enumerate() {
                print_expr(out, w, &format!("arg{}", i), a);
            }
            w.pop();
        }
        Expr::Index { target, index, .. } => {
            w.push();
            print_expr(out, w, "target", target);
            print_expr(out, w, "index", index);
            w.pop();
        }
        Expr::Field { target, name, .. } => {
            w.push();
            print_expr(out, w, "target", target);
            let _ = writeln!(out, "{}field: {}", w.ind(), name);
            w.pop();
        }
        Expr::ActorSend { actor, msg, .. } => {
            w.push();
            print_expr(out, w, "actor", actor);
            print_expr(out, w, "msg", msg);
            w.pop();
        }
        Expr::ActorRequest { actor, msg, .. } => {
            w.push();
            print_expr(out, w, "actor", actor);
            print_expr(out, w, "msg", msg);
            w.pop();
        }
        Expr::Spawn { name, args, .. } => {
            w.push();
            let _ = writeln!(out, "{}actor: {}", w.ind(), name);
            for (i, a) in args.iter().enumerate() {
                print_expr(out, w, &format!("arg{}", i), a);
            }
            w.pop();
        }
        Expr::Raise { expr, .. } => {
            w.push();
            print_expr(out, w, "value", expr);
            w.pop();
        }
        Expr::Try {
            body,
            rescues,
            ensure,
            ..
        } => {
            w.push();
            print_expr(out, w, "body", body);
            for r in rescues {
                let ty = r.type_name.clone().unwrap_or_else(|| "_".into());
                let bind = r.bind.clone().unwrap_or_else(|| "_".into());
                let _ = writeln!(out, "{}rescue {} as {}", w.ind(), ty, bind);
                w.push();
                print_expr(out, w, "body", &r.body);
                w.pop();
            }
            if let Some(e) = ensure {
                print_expr(out, w, "ensure", e);
            }
            w.pop();
        }
        Expr::Transact { body, .. } => {
            w.push();
            print_expr(out, w, "body", body);
            w.pop();
        }
        Expr::Retry { .. } => {}
        Expr::Return { value, .. } => {
            if let Some(v) = value {
                w.push();
                print_expr(out, w, "value", v);
                w.pop();
            }
        }
        Expr::Reply { value, .. } => {
            w.push();
            print_expr(out, w, "value", value);
            w.pop();
        }
        Expr::Yield { .. } => {}
        Expr::Await { expr, .. } => {
            w.push();
            print_expr(out, w, "expr", expr);
            w.pop();
        }
        Expr::SharedExpr { expr, .. } => {
            w.push();
            print_expr(out, w, "expr", expr);
            w.pop();
        }
        Expr::Assign { target, value, .. } => {
            w.push();
            print_expr(out, w, "target", target);
            print_expr(out, w, "value", value);
            w.pop();
        }
        Expr::CompoundAssign { op, target, value, .. } => {
            w.push();
            let _ = writeln!(out, "{}op: {}", w.ind(), op.as_str());
            print_expr(out, w, "target", target);
            print_expr(out, w, "value", value);
            w.pop();
        }
        Expr::While { cond, body, .. } => {
            w.push();
            print_expr(out, w, "cond", cond);
            print_expr(out, w, "body", body);
            w.pop();
        }
        Expr::For { var, iter, body, .. } => {
            w.push();
            let _ = writeln!(out, "{}var: {}", w.ind(), var);
            print_expr(out, w, "iter", iter);
            print_expr(out, w, "body", body);
            w.pop();
        }
        Expr::Loop { body, .. } => {
            w.push();
            print_expr(out, w, "body", body);
            w.pop();
        }
        Expr::Break { value, .. } => {
            if let Some(v) = value {
                w.push();
                print_expr(out, w, "value", v);
                w.pop();
            }
        }
        Expr::Continue { .. } => {}
    }
}

fn expr_head(e: &Expr) -> String {
    match e {
        Expr::Int(n, _) => format!("Int({})", n),
        Expr::Decimal(d, _) => format!("Decimal({})", decimal_str(d)),
        Expr::Str(s, _) => format!("Str({} parts)", s.parts.len()),
        Expr::Bool(b, _) => format!("Bool({})", b),
        Expr::Nil(_) => "Nil".into(),
        Expr::Ident(n, _) => format!("Ident({})", n),
        Expr::VecLit { .. } => "VecLit".into(),
        Expr::MapLit { .. } => "MapLit".into(),
        Expr::SetLit { .. } => "SetLit".into(),
        Expr::BinOp { op, .. } => format!("BinOp({})", op.as_str()),
        Expr::UnaryOp { op, .. } => format!("UnaryOp({:?})", op),
        Expr::Pipe { .. } => "Pipe".into(),
        Expr::If { .. } => "If".into(),
        Expr::Match { .. } => "Match".into(),
        Expr::Lambda { .. } => "Lambda".into(),
        Expr::Block { .. } => "Block".into(),
        Expr::Paren(_, _) => "Paren".into(),
        Expr::Call { .. } => "Call".into(),
        Expr::MethodCall { method, .. } => format!("MethodCall({})", method),
        Expr::Index { .. } => "Index".into(),
        Expr::Field { name, .. } => format!("Field({})", name),
        Expr::ActorSend { .. } => "ActorSend".into(),
        Expr::ActorRequest { .. } => "ActorRequest".into(),
        Expr::Spawn { name, .. } => format!("Spawn({})", name),
        Expr::Raise { .. } => "Raise".into(),
        Expr::Try { .. } => "Try".into(),
        Expr::Transact { .. } => "Transact".into(),
        Expr::Retry { .. } => "Retry".into(),
        Expr::Return { .. } => "Return".into(),
        Expr::Reply { .. } => "Reply".into(),
        Expr::Yield { .. } => "Yield".into(),
        Expr::Await { .. } => "Await".into(),
        Expr::SharedExpr { .. } => "SharedExpr".into(),
        Expr::Assign { .. } => "Assign".into(),
        Expr::CompoundAssign { op, .. } => format!("CompoundAssign({})", op.as_str()),
        Expr::While { .. } => "While".into(),
        Expr::For { .. } => "For".into(),
        Expr::Loop { .. } => "Loop".into(),
        Expr::Break { .. } => "Break".into(),
        Expr::Continue { .. } => "Continue".into(),
    }
}

fn decimal_str(d: &BigDecimal) -> String {
    format!("{}", d)
}

fn print_type(t: &TypeAnnot) -> String {
    match t {
        TypeAnnot::Name { name, .. } => name.clone(),
        TypeAnnot::Generic { name, args, .. } => {
            let a = args.iter().map(print_type).collect::<Vec<_>>().join(", ");
            format!("{}<{}>", name, a)
        }
        TypeAnnot::Union { variants, .. } => variants
            .iter()
            .map(print_type)
            .collect::<Vec<_>>()
            .join(" | "),
        TypeAnnot::Fn { params, ret, .. } => {
            let p = params.iter().map(print_type).collect::<Vec<_>>().join(", ");
            format!("fn({}) -> {}", p, print_type(ret))
        }
    }
}

fn print_pattern(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard(_) => "_".into(),
        Pattern::Lit(l, _) => match l {
            LitPattern::Int(n) => format!("{}", n),
            LitPattern::Decimal(d) => decimal_str(d),
            LitPattern::Str(_) => "<str>".into(),
            LitPattern::Bool(b) => format!("{}", b),
            LitPattern::Nil => "nil".into(),
        },
        Pattern::Bind(n, _) => n.clone(),
        Pattern::Variant { name, args, .. } => {
            if args.is_empty() {
                name.clone()
            } else {
                let a = args.iter().map(print_pattern).collect::<Vec<_>>().join(", ");
                format!("{}({})", name, a)
            }
        }
        Pattern::Struct { fields, rest, .. } => {
            let f = fields
                .iter()
                .map(|(n, p)| format!("{}: {}", n, print_pattern(p)))
                .collect::<Vec<_>>()
                .join(", ");
            if *rest {
                format!("{{ {}, .. }}", f)
            } else {
                format!("{{ {} }}", f)
            }
        }
        Pattern::Vec { pats, rest, .. } => {
            let mut s = String::from("[");
            s.push_str(&pats.iter().map(print_pattern).collect::<Vec<_>>().join(", "));
            if *rest {
                if !pats.is_empty() {
                    s.push_str(", ");
                }
                s.push_str("..");
            }
            s.push(']');
            s
        }
        Pattern::Or(alts, _) => alts.iter().map(print_pattern).collect::<Vec<_>>().join(" | "),
        Pattern::Guard(p, _, _) => format!("{} if ...", print_pattern(p)),
    }
}
