// Type representation + best-effort type inference for the 1y language server.
//
// 1y is dynamically typed, but type annotations (`let x: Int`, `fn foo(n: Int)
// -> Bool`, `type Point = { x: Int, y: Int }`) and literal structure let us
// infer types for hover tooltips, completion filtering, and signature help.
//
// This is *not* a type checker — it never produces errors. When inference
// can't determine a type, it returns `Unknown` and the LSP simply shows less
// information. The inference is conservative: it only follows annotations and
// direct literal structure, not full data-flow analysis.

import { Expr, TypeAnnot, Program, Stmt } from "./ast";
import { SymbolTable, SymbolInfo, resolveAt } from "./symbols";

// ---------------------------------------------------------------------------
// Type representation
// ---------------------------------------------------------------------------

export type Type =
    | { kind: "Int" }
    | { kind: "Decimal" }
    | { kind: "String" }
    | { kind: "Bool" }
    | { kind: "Nil" }
    | { kind: "Vec"; elem: Type }
    | { kind: "Map"; key: Type; val: Type }
    | { kind: "Set"; elem: Type }
    | { kind: "Fn"; params: Type[]; ret: Type }
    | { kind: "User"; name: string }
    | { kind: "Unknown" };

/** Render a Type to display text, e.g. `Vec<Int>`, `fn(Int) -> Bool`. */
export function typeToText(t: Type): string {
    switch (t.kind) {
        case "Int": return "Int";
        case "Decimal": return "Decimal";
        case "String": return "String";
        case "Bool": return "Bool";
        case "Nil": return "Nil";
        case "Vec": return `Vec<${typeToText(t.elem)}>`;
        case "Map": return `Map<${typeToText(t.key)}, ${typeToText(t.val)}>`;
        case "Set": return `Set<${typeToText(t.elem)}>`;
        case "Fn": return `fn(${t.params.map(typeToText).join(", ")}) -> ${typeToText(t.ret)}`;
        case "User": return t.name;
        case "Unknown": return "?";
    }
}

/** Convert a user-written TypeAnnot to an inferred Type. */
export function annotToType(t: TypeAnnot): Type {
    switch (t.kind) {
        case "Name": {
            switch (t.name) {
                case "Int": return { kind: "Int" };
                case "Decimal": return { kind: "Decimal" };
                case "String": return { kind: "String" };
                case "Bool": return { kind: "Bool" };
                case "Nil": return { kind: "Nil" };
                default: return { kind: "User", name: t.name };
            }
        }
        case "Generic": {
            switch (t.name) {
                case "Vec":
                    return { kind: "Vec", elem: t.args[0] ? annotToType(t.args[0]) : { kind: "Unknown" } };
                case "Map":
                    return {
                        kind: "Map",
                        key: t.args[0] ? annotToType(t.args[0]) : { kind: "Unknown" },
                        val: t.args[1] ? annotToType(t.args[1]) : { kind: "Unknown" },
                    };
                case "Set":
                    return { kind: "Set", elem: t.args[0] ? annotToType(t.args[0]) : { kind: "Unknown" } };
                default:
                    return { kind: "User", name: t.name };
            }
        }
        case "Fn":
            return { kind: "Fn", params: t.params.map(annotToType), ret: annotToType(t.ret) };
        case "Union":
            // Unions (Int | Nil) are complex — represent as Unknown for now.
            return { kind: "Unknown" };
    }
}

// ---------------------------------------------------------------------------
// Type map — inferred types for declarations without annotations
// ---------------------------------------------------------------------------

export interface TypeMap {
    /** nameSpan.start → inferred Type (for let/param/func without annotation). */
    inferred: Map<number, Type>;
}

// ---------------------------------------------------------------------------
// Builtin function return types — lets `let n = count(v)` infer `Int`.
// Keyed by builtin name. Where the return depends on an argument's element
// type (e.g. `map`, `filter`), we fall back to Unknown rather than doing
// full data-flow analysis.
// ---------------------------------------------------------------------------

export const BUILTIN_RETURNS: Record<string, () => Type> = {
    println: () => ({ kind: "Nil" }),
    print: () => ({ kind: "Nil" }),
    count: () => ({ kind: "Int" }),
    len: () => ({ kind: "Int" }),
    first: () => ({ kind: "Unknown" }),
    rest: () => ({ kind: "Vec", elem: { kind: "Unknown" } }),
    push: () => ({ kind: "Vec", elem: { kind: "Unknown" } }),
    cons: () => ({ kind: "Vec", elem: { kind: "Unknown" } }),
    assoc: () => ({ kind: "Map", key: { kind: "Unknown" }, val: { kind: "Unknown" } }),
    dissoc: () => ({ kind: "Map", key: { kind: "Unknown" }, val: { kind: "Unknown" } }),
    get: () => ({ kind: "Unknown" }),
    map: () => ({ kind: "Vec", elem: { kind: "Unknown" } }),
    filter: () => ({ kind: "Vec", elem: { kind: "Unknown" } }),
    fold: () => ({ kind: "Unknown" }),
    reduce: () => ({ kind: "Unknown" }),
    find: () => ({ kind: "Unknown" }),
    each: () => ({ kind: "Nil" }),
    split: () => ({ kind: "Vec", elem: { kind: "String" } }),
    join: () => ({ kind: "String" }),
    replace: () => ({ kind: "String" }),
    trim: () => ({ kind: "String" }),
    contains: () => ({ kind: "Bool" }),
    substring: () => ({ kind: "String" }),
    str: () => ({ kind: "String" }),
    int: () => ({ kind: "Int" }),
    pow: () => ({ kind: "Int" }),
    abs: () => ({ kind: "Int" }),
    min: () => ({ kind: "Unknown" }),
    max: () => ({ kind: "Unknown" }),
    sqrt: () => ({ kind: "Decimal" }),
    // Common module-function returns (used when callee is `module.func`).
    // Keys are `module.func` to disambiguate.
    "json.parse": () => ({ kind: "Unknown" }),
    "json.stringify": () => ({ kind: "String" }),
    "json.pretty": () => ({ kind: "String" }),
    "io.read_line": () => ({ kind: "String" }),
    "io.read_to_string": () => ({ kind: "String" }),
    "io.write": () => ({ kind: "Nil" }),
    "io.append": () => ({ kind: "Nil" }),
    "io.exists": () => ({ kind: "Bool" }),
    "env.get": () => ({ kind: "String" }),
    "env.set": () => ({ kind: "Nil" }),
    "env.unset": () => ({ kind: "Nil" }),
    "env.args": () => ({ kind: "Vec", elem: { kind: "String" } }),
    "env.vars": () => ({ kind: "Map", key: { kind: "String" }, val: { kind: "String" } }),
};

// ---------------------------------------------------------------------------
// Method return types — for `receiver.method(args)` where the receiver's
// type is known. Keyed by `TypeKind.method`. Where the method preserves the
// receiver type (e.g. Vec.push), we mirror the receiver type.
// ---------------------------------------------------------------------------

function methodReturnType(receiver: Type, method: string): Type | null {
    // String methods
    if (receiver.kind === "String") {
        switch (method) {
            case "len": return { kind: "Int" };
            case "split": return { kind: "Vec", elem: { kind: "String" } };
            case "replace": case "trim": case "substring": return { kind: "String" };
            case "contains": return { kind: "Bool" };
        }
    }
    // Vec methods
    if (receiver.kind === "Vec") {
        switch (method) {
            case "len": case "count": return { kind: "Int" };
            case "first": return receiver.elem;
            case "rest": return receiver;
            case "push": return receiver;
            case "contains": return { kind: "Bool" };
            case "map": case "filter": return { kind: "Vec", elem: { kind: "Unknown" } };
            case "each": return { kind: "Nil" };
        }
    }
    // Map methods
    if (receiver.kind === "Map") {
        switch (method) {
            case "len": case "count": return { kind: "Int" };
            case "keys": return { kind: "Vec", elem: receiver.key };
            case "values": return { kind: "Vec", elem: receiver.val };
            case "get": return receiver.val;
            case "contains": case "has": return { kind: "Bool" };
        }
    }
    // Set methods
    if (receiver.kind === "Set") {
        switch (method) {
            case "len": case "count": return { kind: "Int" };
            case "contains": case "has": return { kind: "Bool" };
        }
    }
    return null;
}

// ---------------------------------------------------------------------------
// Parameter-sensitive builtin / method inference
// ---------------------------------------------------------------------------

/**
 * Infer the return type of a parameter-sensitive builtin function call.
 * Returns null if the function isn't parameter-sensitive (caller falls back
 * to the fixed-return table or user-defined resolution).
 *
 *   push(v, x)   → Vec<v.elem>        (v is Vec)
 *   cons(x, v)   → Vec<v.elem>        (v is Vec, x prepended)
 *   first(v)     → v.elem             (v is Vec)
 *   rest(v)      → v                  (v is Vec)
 *   map(f, v)    → Vec<f.ret>         (f is fn, v is Vec)
 *   filter(f, v) → v                  (v is Vec)
 *   fold(f, init, v) → init           (init type)
 *   reduce(f, init, v) → init
 *   get(m, k)    → m.val              (m is Map)
 *   find(f, v)   → v.elem | Unknown   (v is Vec, best-effort)
 *   min(a, b) / max(a, b) → a         (same type as first arg)
 *   abs(n)       → n                  (same type as arg)
 *   str(x)       → String             (fixed, but listed here for clarity)
 */
function inferBuiltinCall(name: string, argTypes: Type[]): Type | null {
    switch (name) {
        case "push":
        case "cons":
            // push(v, x) / cons(x, v): result is a Vec with the same element
            // type as the Vec argument. Find the Vec among the args.
            for (const t of argTypes) {
                if (t.kind === "Vec") return t;
            }
            return { kind: "Vec", elem: { kind: "Unknown" } };

        case "first":
            if (argTypes[0] && argTypes[0].kind === "Vec") return argTypes[0].elem;
            return null;

        case "rest":
            if (argTypes[0] && argTypes[0].kind === "Vec") return argTypes[0];
            return null;

        case "map": {
            // map(f, v): Vec<f.ret>. f is the first arg, v the second.
            const fType = argTypes[0];
            const vType = argTypes[1];
            if (fType && fType.kind === "Fn") {
                return { kind: "Vec", elem: fType.ret };
            }
            if (vType && vType.kind === "Vec") {
                return { kind: "Vec", elem: { kind: "Unknown" } };
            }
            return null;
        }

        case "filter":
            // filter(f, v): same type as v.
            if (argTypes[1]) return argTypes[1];
            return null;

        case "fold":
        case "reduce":
            // fold(f, init, v): result type is init's type (2nd arg).
            if (argTypes[1]) return argTypes[1];
            return null;

        case "get":
            // get(m, k): m's value type.
            if (argTypes[0] && argTypes[0].kind === "Map") return argTypes[0].val;
            return null;

        case "find":
            // find(f, v): element type of v, or Unknown.
            if (argTypes[1] && argTypes[1].kind === "Vec") return argTypes[1].elem;
            return null;

        case "min":
        case "max":
            // min(a, b): same type as first arg.
            if (argTypes[0]) return argTypes[0];
            return null;

        case "abs":
            // abs(n): same type as arg (Int or Decimal).
            if (argTypes[0]) return argTypes[0];
            return null;
    }
    return null;
}

/**
 * Infer the return type of a parameter-sensitive method call. Returns null
 * if the method isn't parameter-sensitive (caller falls back to the
 * fixed-return table).
 *
 *   v.push(x)    → Vec<v.elem>
 *   v.cons(x)    → Vec<v.elem>
 *   v.first()    → v.elem
 *   v.rest()     → v
 *   v.map(f)     → Vec<f.ret>
 *   v.filter(f)  → v
 *   v.fold(init, f) → init
 *   m.get(k)     → m.val
 *   m.keys()     → Vec<m.key>  (fixed, but listed for clarity)
 */
function inferMethodCall(receiver: Type, method: string, argTypes: Type[]): Type | null {
    switch (method) {
        case "push":
        case "cons":
            if (receiver.kind === "Vec") return receiver;
            return null;

        case "first":
            if (receiver.kind === "Vec") return receiver.elem;
            return null;

        case "rest":
            if (receiver.kind === "Vec") return receiver;
            return null;

        case "map": {
            // v.map(f): Vec<f.ret>.
            if (argTypes[0] && argTypes[0].kind === "Fn") {
                return { kind: "Vec", elem: argTypes[0].ret };
            }
            if (receiver.kind === "Vec") {
                return { kind: "Vec", elem: { kind: "Unknown" } };
            }
            return null;
        }

        case "filter":
            // v.filter(f): same type as receiver.
            return receiver.kind === "Vec" ? receiver : null;

        case "fold":
        case "reduce":
            // v.fold(init, f): init's type.
            if (argTypes[0]) return argTypes[0];
            return null;

        case "get":
            if (receiver.kind === "Map") return receiver.val;
            return null;

        case "find":
            if (receiver.kind === "Vec") return receiver.elem;
            return null;
    }
    return null;
}

/**
 * Walk the AST and infer types for declarations that lack annotations:
 *   - `let x = 42`        → Int
 *   - `let s = "hi"`      → String
 *   - `let v = [1, 2]`    → Vec<Int>
 *   - `fn f(n) { n + 1 }` → fn(Int) -> Int (from body inference)
 *
 * Uses the symbol table for identifier resolution during inference.
 */
export function buildTypeMap(program: Program, table: SymbolTable): TypeMap {
    const inferred = new Map<number, Type>();
    walkStmts(program.stmts, table, inferred, 0);
    return { inferred };
}

/** Get the type of a symbol: annotation first, then inferred, else Unknown. */
export function symbolType(sym: SymbolInfo, typeMap: TypeMap): Type {
    if (sym.typeAnnot) return annotToType(sym.typeAnnot);

    if (sym.kind === "type" || sym.kind === "enum" || sym.kind === "actor") {
        return { kind: "User", name: sym.name };
    }

    if (sym.kind === "variant") {
        if (sym.variantArgs && sym.variantArgs.length > 0) {
            return {
                kind: "Fn",
                params: sym.variantArgs.map(annotToType),
                ret: { kind: "User", name: sym.name },
            };
        }
        return { kind: "User", name: sym.name };
    }

    // For functions / handlers: construct a proper fn(...) -> T type.
    // The inferred map stores the *body* type (= return type), so we wrap
    // it with the param types to get the full function type. This lets
    // `Call` inference extract `.ret` correctly.
    if (sym.kind === "function" || sym.kind === "on") {
        const retAnnot = sym.returnType ? annotToType(sym.returnType) : null;
        const retType = retAnnot ?? typeMap.inferred.get(sym.declSpan.start) ?? { kind: "Unknown" };
        const paramTypes: Type[] = (sym.params || []).map((p) =>
            p.typeAnnot ? annotToType(p.typeAnnot) : { kind: "Unknown" },
        );
        return { kind: "Fn", params: paramTypes, ret: retType };
    }

    // For let / shared / state / param / etc.: use the inferred type map.
    const t = typeMap.inferred.get(sym.declSpan.start);
    return t ?? { kind: "Unknown" };
}

// ---------------------------------------------------------------------------
// Expression type inference
// ---------------------------------------------------------------------------

/**
 * Infer the type of an expression. `pos` is a source position within the
 * expression's scope, used for lexical symbol resolution.
 */
export function inferExpr(
    expr: Expr,
    table: SymbolTable,
    typeMap: TypeMap,
    pos: number,
): Type {
    switch (expr.kind) {
        case "Int": return { kind: "Int" };
        case "Decimal": return { kind: "Decimal" };
        case "Str": return { kind: "String" };
        case "Bool": return { kind: "Bool" };
        case "Nil": return { kind: "Nil" };

        case "Ident": {
            const sym = resolveAt(table, pos, expr.name);
            if (!sym) return { kind: "Unknown" };
            return symbolType(sym, typeMap);
        }

        case "BinOp": {
            switch (expr.op) {
                case "Add": {
                    const lt = inferExpr(expr.lhs, table, typeMap, pos);
                    const rt = inferExpr(expr.rhs, table, typeMap, pos);
                    if (lt.kind === "String" || rt.kind === "String") return { kind: "String" };
                    if (lt.kind === "Decimal" || rt.kind === "Decimal") return { kind: "Decimal" };
                    if (lt.kind === "Unknown" || rt.kind === "Unknown") return { kind: "Unknown" };
                    return { kind: "Int" };
                }
                case "Sub":
                case "Mul":
                case "Mod": {
                    const l = inferExpr(expr.lhs, table, typeMap, pos);
                    const r = inferExpr(expr.rhs, table, typeMap, pos);
                    if (l.kind === "Decimal" || r.kind === "Decimal") return { kind: "Decimal" };
                    if (l.kind === "Unknown" || r.kind === "Unknown") return { kind: "Unknown" };
                    return { kind: "Int" };
                }
                case "Div":
                    // True division promotes to Decimal (even Int / Int).
                    return { kind: "Decimal" };
                case "Eq": case "Neq": case "Lt": case "Gt": case "Lte": case "Gte":
                case "And": case "Or":
                    return { kind: "Bool" };
            }
            return { kind: "Unknown" };
        }

        case "UnaryOp":
            if (expr.op === "Neg") return inferExpr(expr.expr, table, typeMap, pos);
            return { kind: "Bool" }; // Not

        case "Pipe":
            // `a |> f(b)` ≡ `f(a, b)` — result type is the rhs call's type.
            return inferExpr(expr.rhs, table, typeMap, pos);

        case "Paren":
            return inferExpr(expr.expr, table, typeMap, pos);

        case "Call": {
            // If the callee is a bare identifier naming a builtin, consult
            // the builtin return-type table. This lets `let n = count(v)`
            // infer `Int` even though `count` has no user declaration.
            if (expr.callee.kind === "Ident") {
                const name = expr.callee.name;
                // Parameter-sensitive builtins: infer from arguments.
                const argTypes = expr.args.map((a) => inferExpr(a, table, typeMap, pos));
                const sensitive = inferBuiltinCall(name, argTypes);
                if (sensitive) return sensitive;
                // Fixed-return builtins.
                if (BUILTIN_RETURNS[name]) return BUILTIN_RETURNS[name]();
            }
            // If the callee is a `module.func` field access and the module
            // is a builtin (e.g. `json.stringify`), consult the table too.
            if (expr.callee.kind === "Field") {
                const tgt = expr.callee.target;
                if (tgt.kind === "Ident") {
                    // Resolve the target as an import alias to recover the
                    // module name, then look up `module.func`.
                    const sym = resolveAt(table, pos, tgt.name);
                    if (sym && sym.kind === "import" && sym.modulePath) {
                        const modName = sym.modulePath.split(".").pop() || sym.modulePath;
                        const key = `${modName}.${expr.callee.name}`;
                        if (BUILTIN_RETURNS[key]) return BUILTIN_RETURNS[key]();
                    }
                    // Also try the literal module name (when the user wrote
                    // `json.stringify` directly without importing).
                    const key = `${tgt.name}.${expr.callee.name}`;
                    if (BUILTIN_RETURNS[key]) return BUILTIN_RETURNS[key]();
                }
            }
            const calleeType = inferExpr(expr.callee, table, typeMap, pos);
            if (calleeType.kind === "Fn") return calleeType.ret;
            return { kind: "Unknown" };
        }

        case "MethodCall": {
            const receiverType = inferExpr(expr.receiver, table, typeMap, pos);
            // Parameter-sensitive method calls: infer from receiver + args.
            const argTypes = expr.args.map((a) => inferExpr(a, table, typeMap, pos));
            const sensitive = inferMethodCall(receiverType, expr.method, argTypes);
            if (sensitive) return sensitive;
            // Fixed-return methods.
            const t = methodReturnType(receiverType, expr.method);
            return t ?? { kind: "Unknown" };
        }

        case "Field": {
            const targetType = inferExpr(expr.target, table, typeMap, pos);
            if (targetType.kind === "User") {
                const sym = resolveAt(table, pos, targetType.name);
                if (sym && sym.kind === "type" && sym.fields) {
                    const field = sym.fields.find((f) => f.name === expr.name);
                    if (field) return annotToType(field.type);
                }
            }
            return { kind: "Unknown" };
        }

        case "Index": {
            const targetType = inferExpr(expr.target, table, typeMap, pos);
            if (targetType.kind === "Vec") return targetType.elem;
            if (targetType.kind === "Map") return targetType.val;
            return { kind: "Unknown" };
        }

        case "VecLit": {
            if (expr.items.length === 0) return { kind: "Vec", elem: { kind: "Unknown" } };
            return { kind: "Vec", elem: inferExpr(expr.items[0], table, typeMap, pos) };
        }

        case "MapLit": {
            if (expr.entries.length === 0) {
                return { kind: "Map", key: { kind: "Unknown" }, val: { kind: "Unknown" } };
            }
            return {
                kind: "Map",
                key: inferExpr(expr.entries[0].key, table, typeMap, pos),
                val: inferExpr(expr.entries[0].value, table, typeMap, pos),
            };
        }

        case "SetLit": {
            if (expr.items.length === 0) return { kind: "Set", elem: { kind: "Unknown" } };
            return { kind: "Set", elem: inferExpr(expr.items[0], table, typeMap, pos) };
        }

        case "Lambda": {
            const paramTypes: Type[] = expr.params.map((p) =>
                p.typeAnnot ? annotToType(p.typeAnnot) : { kind: "Unknown" },
            );
            const retType = expr.returnType
                ? annotToType(expr.returnType)
                : inferExpr(expr.body, table, typeMap, pos);
            return { kind: "Fn", params: paramTypes, ret: retType };
        }

        case "Block":
            // Use the tail's own position so inner-scope let bindings (which
            // are visible only after their declaration) resolve correctly.
            return expr.tail
                ? inferExpr(expr.tail, table, typeMap, expr.tail.span.start)
                : { kind: "Nil" };

        case "If": {
            const thenType = inferExpr(expr.then, table, typeMap, pos);
            if (expr.else_) {
                const elseType = inferExpr(expr.else_, table, typeMap, pos);
                return typesEqual(thenType, elseType) ? thenType : { kind: "Unknown" };
            }
            return { kind: "Nil" };
        }

        case "Match":
            return expr.arms.length > 0
                ? inferExpr(expr.arms[0].body, table, typeMap, pos)
                : { kind: "Nil" };

        case "While":
        case "Loop":
        case "For":
        case "Break":
        case "Continue":
        case "Return":
        case "Reply":
        case "Yield":
        case "Retry":
        case "Raise":
        case "Assign":
        case "CompoundAssign":
        case "ActorSend":
            return { kind: "Nil" };

        case "Await":
            return inferExpr(expr.expr, table, typeMap, pos);

        case "SharedExpr":
            return { kind: "Unknown" };

        case "Transact":
            return inferExpr(expr.body, table, typeMap, pos);

        case "Try":
            return inferExpr(expr.body, table, typeMap, pos);

        case "Spawn":
            return { kind: "User", name: "ActorPid" };

        case "ActorRequest":
            return { kind: "Unknown" };
    }
    return { kind: "Unknown" };
}

// ---------------------------------------------------------------------------
// Internal: AST walk to populate the inferred type map
// ---------------------------------------------------------------------------

function walkStmts(
    stmts: Stmt[],
    table: SymbolTable,
    inferred: Map<number, Type>,
    scopePos: number,
): void {
    for (const s of stmts) walkStmt(s, table, inferred, scopePos);
}

function walkStmt(
    stmt: Stmt,
    table: SymbolTable,
    inferred: Map<number, Type>,
    scopePos: number,
): void {
    switch (stmt.kind) {
        case "FuncDef": {
            // Set a placeholder first so recursive self-references resolve to
            // Unknown instead of failing. Then walk the body (populating types
            // of inner declarations), and finally infer the return type.
            if (!stmt.returnType) {
                inferred.set(stmt.span.start, { kind: "Unknown" });
                walkExpr(stmt.body, table, inferred, stmt.span.start);
                const bodyType = inferExpr(stmt.body, table, { inferred }, stmt.span.start);
                inferred.set(stmt.span.start, bodyType);
            } else {
                walkExpr(stmt.body, table, inferred, stmt.span.start);
            }
            break;
        }

        case "OnClause": {
            if (!stmt.returnType) {
                inferred.set(stmt.span.start, { kind: "Unknown" });
                walkExpr(stmt.body, table, inferred, stmt.span.start);
                const bodyType = inferExpr(stmt.body, table, { inferred }, stmt.span.start);
                inferred.set(stmt.span.start, bodyType);
            } else {
                walkExpr(stmt.body, table, inferred, stmt.span.start);
            }
            break;
        }

        case "ActorDef":
            for (const s of stmt.body) walkStmt(s, table, inferred, stmt.span.start);
            break;

        case "TypeDef":
        case "EnumDef":
        case "Import":
            break; // no type inference needed

        case "Let":
            if (!stmt.typeAnnot) {
                // Walk the value first so inner declarations (e.g. `let a` in
                // `let r = { let a = 10; a }`) have their types populated
                // before we infer the outer binding's type from the value.
                walkExpr(stmt.value, table, inferred, stmt.span.start);
                const vp = stmt.value.span.start;
                const valType = inferExpr(stmt.value, table, { inferred }, vp);
                inferred.set(stmt.span.start, valType);
            } else {
                walkExpr(stmt.value, table, inferred, stmt.span.start);
            }
            break;

        case "SharedDecl":
            if (!stmt.typeAnnot) {
                walkExpr(stmt.value, table, inferred, stmt.span.start);
                const vp = stmt.value.span.start;
                const valType = inferExpr(stmt.value, table, { inferred }, vp);
                inferred.set(stmt.span.start, valType);
            } else {
                walkExpr(stmt.value, table, inferred, stmt.span.start);
            }
            break;

        case "StateDecl":
            if (!stmt.typeAnnot) {
                walkExpr(stmt.value, table, inferred, stmt.span.start);
                const vp = stmt.value.span.start;
                const valType = inferExpr(stmt.value, table, { inferred }, vp);
                inferred.set(stmt.span.start, valType);
            } else {
                walkExpr(stmt.value, table, inferred, stmt.span.start);
            }
            break;

        case "Expr":
            walkExpr(stmt.expr, table, inferred, scopePos);
            break;

        case "Semi":
            walkExpr(stmt.expr, table, inferred, scopePos);
            break;
    }
}

function walkExpr(
    expr: Expr,
    table: SymbolTable,
    inferred: Map<number, Type>,
    scopePos: number,
): void {
    switch (expr.kind) {
        case "Block":
            for (const s of expr.stmts) walkStmt(s, table, inferred, expr.span.start);
            if (expr.tail) walkExpr(expr.tail, table, inferred, expr.span.start);
            break;
        case "Lambda":
            walkExpr(expr.body, table, inferred, expr.span.start);
            break;
        case "If":
            walkExpr(expr.cond, table, inferred, scopePos);
            walkExpr(expr.then, table, inferred, scopePos);
            if (expr.else_) walkExpr(expr.else_, table, inferred, scopePos);
            break;
        case "Match":
            walkExpr(expr.scrutinee, table, inferred, scopePos);
            for (const arm of expr.arms) {
                if (arm.guard) walkExpr(arm.guard, table, inferred, arm.span.start);
                walkExpr(arm.body, table, inferred, arm.span.start);
            }
            break;
        case "BinOp": case "Pipe":
            walkExpr(expr.lhs, table, inferred, scopePos);
            walkExpr(expr.rhs, table, inferred, scopePos);
            break;
        case "UnaryOp": case "Paren": case "Raise": case "Await":
        case "SharedExpr":
            walkExpr(expr.expr, table, inferred, scopePos);
            break;
        case "Transact": case "Loop":
            walkExpr(expr.body, table, inferred, scopePos);
            break;
        case "Call":
            walkExpr(expr.callee, table, inferred, scopePos);
            for (const a of expr.args) walkExpr(a, table, inferred, scopePos);
            break;
        case "MethodCall":
            walkExpr(expr.receiver, table, inferred, scopePos);
            for (const a of expr.args) walkExpr(a, table, inferred, scopePos);
            break;
        case "Index":
            walkExpr(expr.target, table, inferred, scopePos);
            walkExpr(expr.index, table, inferred, scopePos);
            break;
        case "Field":
            walkExpr(expr.target, table, inferred, scopePos);
            break;
        case "VecLit": case "SetLit":
            for (const i of expr.items) walkExpr(i, table, inferred, scopePos);
            break;
        case "MapLit":
            for (const e of expr.entries) {
                walkExpr(e.key, table, inferred, scopePos);
                walkExpr(e.value, table, inferred, scopePos);
            }
            break;
        case "Assign": case "CompoundAssign":
            walkExpr(expr.target, table, inferred, scopePos);
            walkExpr(expr.value, table, inferred, scopePos);
            break;
        case "While":
            walkExpr(expr.cond, table, inferred, scopePos);
            walkExpr(expr.body, table, inferred, scopePos);
            break;
        case "For":
            walkExpr(expr.iter, table, inferred, scopePos);
            walkExpr(expr.body, table, inferred, scopePos);
            break;
        case "Break":
            if (expr.value) walkExpr(expr.value, table, inferred, scopePos);
            break;
        case "Return":
            if (expr.value) walkExpr(expr.value, table, inferred, scopePos);
            break;
        case "Reply":
            walkExpr(expr.value, table, inferred, scopePos);
            break;
        case "Try":
            walkExpr(expr.body, table, inferred, scopePos);
            for (const r of expr.rescues) walkExpr(r.body, table, inferred, scopePos);
            if (expr.ensure) walkExpr(expr.ensure, table, inferred, scopePos);
            break;
        case "ActorSend": case "ActorRequest":
            walkExpr(expr.actor, table, inferred, scopePos);
            walkExpr(expr.msg, table, inferred, scopePos);
            break;
        // Literals, Spawn, Yield, Retry, Continue — no sub-exprs.
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function typesEqual(a: Type, b: Type): boolean {
    if (a.kind !== b.kind) return false;
    switch (a.kind) {
        case "Vec": return typesEqual(a.elem, (b as Extract<Type, { kind: "Vec" }>).elem);
        case "Map": {
            const mb = b as Extract<Type, { kind: "Map" }>;
            return typesEqual(a.key, mb.key) && typesEqual(a.val, mb.val);
        }
        case "Set": return typesEqual(a.elem, (b as Extract<Type, { kind: "Set" }>).elem);
        case "Fn": {
            const fb = b as Extract<Type, { kind: "Fn" }>;
            return a.params.length === fb.params.length &&
                a.params.every((p, i) => typesEqual(p, fb.params[i])) &&
                typesEqual(a.ret, fb.ret);
        }
        case "User": return a.name === (b as Extract<Type, { kind: "User" }>).name;
        default: return true;
    }
}
