// Completion data for the 1y language server.
// Provides keyword, builtin, and stdlib function completions.

import { CompletionItem, CompletionItemKind, InsertTextFormat } from "vscode-languageserver-types";

export const KEYWORD_COMPLETIONS: CompletionItem[] = [
    { label: "let", kind: CompletionItemKind.Keyword, detail: "Bind a value", insertText: "let ${1:name} = ${2:expr};", insertTextFormat: InsertTextFormat.Snippet },
    { label: "fn", kind: CompletionItemKind.Keyword, detail: "Declare a function", insertText: "fn ${1:name}(${2:params}) -> ${3:Type} {\n\t${4}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "if", kind: CompletionItemKind.Keyword, detail: "Conditional", insertText: "if ${1:cond} {\n\t${2}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "else", kind: CompletionItemKind.Keyword, detail: "Else branch", insertText: "else {\n\t${1}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "match", kind: CompletionItemKind.Keyword, detail: "Pattern match", insertText: "match ${1:expr} {\n\t${2:Pattern} => ${3:expr},\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "while", kind: CompletionItemKind.Keyword, detail: "While loop", insertText: "while ${1:cond} {\n\t${2}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "loop", kind: CompletionItemKind.Keyword, detail: "Infinite loop", insertText: "loop {\n\t${1}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "for", kind: CompletionItemKind.Keyword, detail: "For loop", insertText: "for ${1:x} in ${2:iter} {\n\t${3}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "break", kind: CompletionItemKind.Keyword, detail: "Break from loop", insertText: "break ${1:value}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "enum", kind: CompletionItemKind.Keyword, detail: "Declare enum", insertText: "enum ${1:Name} {\n\t${2:Variant},\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "type", kind: CompletionItemKind.Keyword, detail: "Declare struct type", insertText: "type ${1:Name} = { ${2:field}: ${3:Type} }", insertTextFormat: InsertTextFormat.Snippet },
    { label: "import", kind: CompletionItemKind.Keyword, detail: "Import module", insertText: "import ${1:module};", insertTextFormat: InsertTextFormat.Snippet },
    { label: "lazy import", kind: CompletionItemKind.Keyword, detail: "Lazy import (deferred load)", insertText: "lazy import ${1:module};", insertTextFormat: InsertTextFormat.Snippet },
    { label: "spawn", kind: CompletionItemKind.Keyword, detail: "Spawn an actor", insertText: "spawn(${1:initial_state}) {\n\t${2}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "receive", kind: CompletionItemKind.Keyword, detail: "Receive a message", insertText: "receive {\n\t${1:Pattern} => ${2:handler},\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "shared", kind: CompletionItemKind.Keyword, detail: "Create a shared cell", insertText: "shared ${1:expr}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "transact", kind: CompletionItemKind.Keyword, detail: "Transactional block", insertText: "transact {\n\t${1}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "try", kind: CompletionItemKind.Keyword, detail: "Try block", insertText: "try {\n\t${1}\n} rescue as ${2:e} {\n\t${3}\n}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "raise", kind: CompletionItemKind.Keyword, detail: "Raise exception", insertText: "raise ${1:expr}", insertTextFormat: InsertTextFormat.Snippet },
    { label: "return", kind: CompletionItemKind.Keyword, detail: "Return from function", insertText: "return ${1:expr};", insertTextFormat: InsertTextFormat.Snippet },
];

export const BUILTIN_COMPLETIONS: CompletionItem[] = [
    { label: "println", kind: CompletionItemKind.Function, detail: "(v?) -> Nil", documentation: "Print value + newline", insertText: "println(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "print", kind: CompletionItemKind.Function, detail: "(v?) -> Nil", documentation: "Print value, no newline", insertText: "print(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "count", kind: CompletionItemKind.Function, detail: "(Vec/Map/Set/Str) -> Int", documentation: "Collection size", insertText: "count(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "first", kind: CompletionItemKind.Function, detail: "(Vec) -> Value", documentation: "First element", insertText: "first(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "rest", kind: CompletionItemKind.Function, detail: "(Vec) -> Vec", documentation: "All but first", insertText: "rest(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "push", kind: CompletionItemKind.Function, detail: "(Vec, Value) -> Vec", documentation: "Append element", insertText: "push(${1:vec}, ${2:value})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "cons", kind: CompletionItemKind.Function, detail: "(Value, Vec) -> Vec", documentation: "Prepend element", insertText: "cons(${1:value}, ${2:vec})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "assoc", kind: CompletionItemKind.Function, detail: "(Map, Key, Val) -> Map", documentation: "Add/update map key", insertText: "assoc(${1:map}, ${2:key}, ${3:value})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "dissoc", kind: CompletionItemKind.Function, detail: "(Map, Key) -> Map", documentation: "Remove map key", insertText: "dissoc(${1:map}, ${2:key})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "get", kind: CompletionItemKind.Function, detail: "(Map, Key) -> Value", documentation: "Lookup map key", insertText: "get(${1:map}, ${2:key})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "map", kind: CompletionItemKind.Function, detail: "(Vec, Func) -> Vec", documentation: "Apply function to each element", insertText: "map(${1:vec}, ${2:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "filter", kind: CompletionItemKind.Function, detail: "(Vec, Func) -> Vec", documentation: "Keep matching elements", insertText: "filter(${1:vec}, ${2:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "fold", kind: CompletionItemKind.Function, detail: "(Vec, Value, Func) -> Value", documentation: "Left fold with init", insertText: "fold(${1:vec}, ${2:init}, ${3:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "reduce", kind: CompletionItemKind.Function, detail: "(Vec, Func) -> Value", documentation: "Left fold, first as init", insertText: "reduce(${1:vec}, ${2:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "find", kind: CompletionItemKind.Function, detail: "(Vec, Func) -> Value or Nil", documentation: "First matching element", insertText: "find(${1:vec}, ${2:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "each", kind: CompletionItemKind.Function, detail: "(Vec, Func) -> Nil", documentation: "Iterate with side effects", insertText: "each(${1:vec}, ${2:fn})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "len", kind: CompletionItemKind.Function, detail: "(Str) -> Int", documentation: "String length", insertText: "len(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "split", kind: CompletionItemKind.Function, detail: "(Str, Str) -> Vec", documentation: "Split string", insertText: "split(${1:str}, ${2:sep})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "join", kind: CompletionItemKind.Function, detail: "(Vec, Str) -> Str", documentation: "Join with separator", insertText: "join(${1:vec}, ${2:sep})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "replace", kind: CompletionItemKind.Function, detail: "(Str, Str, Str) -> Str", documentation: "Replace all occurrences", insertText: "replace(${1:str}, ${2:from}, ${3:to})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "trim", kind: CompletionItemKind.Function, detail: "(Str) -> Str", documentation: "Strip whitespace", insertText: "trim(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "contains", kind: CompletionItemKind.Function, detail: "(Str, Str) -> Bool", documentation: "Substring test", insertText: "contains(${1:str}, ${2:sub})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "substring", kind: CompletionItemKind.Function, detail: "(Str, Int, Int) -> Str", documentation: "Slice [start, end)", insertText: "substring(${1:str}, ${2:start}, ${3:end})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "str", kind: CompletionItemKind.Function, detail: "(Value) -> Str", documentation: "Convert to string", insertText: "str(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "int", kind: CompletionItemKind.Function, detail: "(Value) -> Int", documentation: "Convert to int", insertText: "int(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "pow", kind: CompletionItemKind.Function, detail: "(Int, Int) -> Int", documentation: "Integer power", insertText: "pow(${1:base}, ${2:exp})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "abs", kind: CompletionItemKind.Function, detail: "(Int or Decimal) -> same", documentation: "Absolute value", insertText: "abs(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "min", kind: CompletionItemKind.Function, detail: "(Value...) -> Value", documentation: "Minimum", insertText: "min(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "max", kind: CompletionItemKind.Function, detail: "(Value...) -> Value", documentation: "Maximum", insertText: "max(${1})", insertTextFormat: InsertTextFormat.Snippet },
    { label: "sqrt", kind: CompletionItemKind.Function, detail: "(Value) -> Decimal", documentation: "Square root", insertText: "sqrt(${1})", insertTextFormat: InsertTextFormat.Snippet },
];

export const MODULE_COMPLETIONS: Record<string, CompletionItem[]> = {
    env: makeModuleItems("env", [
        ["get", "(Str) -> Str or Nil", "Get environment variable"],
        ["set", "(Str, Str)", "Set environment variable"],
        ["unset", "(Str)", "Remove environment variable"],
        ["args", "() -> Vec", "Command-line arguments"],
        ["vars", "() -> Map", "All environment variables"],
    ]),
    io: makeModuleItems("io", [
        ["read_line", "() -> Str or Nil", "Read stdin line"],
        ["read_to_string", "(Str) -> Str", "Read file to string"],
        ["write", "(Str, Str)", "Write string to file"],
        ["append", "(Str, Str)", "Append string to file"],
        ["exists", "(Str) -> Bool", "Check file exists"],
    ]),
    json: makeModuleItems("json", [
        ["parse", "(Str) -> Value", "Parse JSON"],
        ["stringify", "(Value) -> Str", "Serialize to JSON"],
        ["pretty", "(Value, Int) -> Str", "Serialize to indented JSON"],
    ]),
    process: makeModuleItems("process", [
        ["exit", "(Int)", "Exit process"],
        ["exec", "(Str, Vec) -> Str", "Run command, return stdout"],
        ["exec_status", "(Str, Vec) -> Int", "Run command, return exit code"],
        ["pid", "() -> Int", "Process ID"],
        ["cwd", "() -> Str", "Current directory"],
        ["set_cwd", "(Str)", "Change directory"],
        ["sleep_ms", "(Int)", "Sleep milliseconds"],
    ]),
    random: makeModuleItems("random", [
        ["int", "(Int) -> Int", "Random in [0, max)"],
        ["range", "(Int, Int) -> Int", "Random in [min, max)"],
        ["float", "() -> Decimal", "Random in [0, 1)"],
        ["bool", "() -> Bool", "Random boolean"],
        ["pick", "(Vec) -> Value", "Random element"],
        ["shuffle", "(Vec) -> Vec", "Shuffle copy"],
        ["seed", "(Int)", "Seed PRNG"],
    ]),
    crypto: makeModuleItems("crypto", [
        ["sha256", "(Str) -> Str", "SHA-256 hash (hex)"],
        ["sha512", "(Str) -> Str", "SHA-512 hash (hex)"],
        ["sha1", "(Str) -> Str", "SHA-1 hash (hex)"],
        ["md5", "(Str) -> Str", "MD5 hash (hex)"],
        ["hmac_sha256", "(Str, Str) -> Str", "HMAC-SHA-256 (hex)"],
        ["hmac_sha512", "(Str, Str) -> Str", "HMAC-SHA-512 (hex)"],
        ["base64_encode", "(Str) -> Str", "Base64 encode"],
        ["base64_decode", "(Str) -> Str", "Base64 decode"],
        ["hex_encode", "(Str) -> Str", "Hex encode"],
        ["hex_decode", "(Str) -> Str", "Hex decode"],
        ["random_bytes", "(Int) -> Vec<Int>", "CSPRNG bytes"],
        ["secure_int", "(Int) -> Int", "CSPRNG int in [0, max)"],
        ["secure_float", "() -> Decimal", "CSPRNG float in [0, 1)"],
    ]),
    socket: makeModuleItems("socket", [
        ["listen", "(Str) -> Opaque", "Bind TCP listener"],
        ["accept", "(Opaque) -> Opaque", "Accept connection"],
        ["connect", "(Str) -> Opaque", "Connect to addr:port"],
        ["read", "(Opaque, Int) -> Str or Nil", "Read up to N bytes"],
        ["read_line", "(Opaque) -> Str or Nil", "Read until newline"],
        ["write", "(Opaque, Str)", "Write string"],
        ["close", "(Opaque)", "Close socket"],
        ["peer_addr", "(Opaque) -> Str", "Remote address"],
    ]),
    tls: makeModuleItems("tls", [
        ["connect", "(Str, Int) -> Opaque", "TLS connect"],
        ["read", "(Opaque, Int) -> Str or Nil", "Read up to N bytes"],
        ["read_line", "(Opaque) -> Str or Nil", "Read until newline"],
        ["write", "(Opaque, Str)", "Write string"],
        ["close", "(Opaque)", "Close TLS stream"],
        ["peer_addr", "(Opaque) -> Str", "Remote address"],
    ]),
    ffi: makeModuleItems("ffi", [
        ["load", "(Str) -> Opaque", "Open shared library"],
        ["call", "(Opaque, Str, Str, Vec) -> Value", "Call foreign function"],
        ["unload", "(Opaque)", "Close library"],
        ["is_loaded", "(Str) -> Bool", "Check file exists"],
    ]),
};

function makeModuleItems(module: string, fns: [string, string, string][]): CompletionItem[] {
    return fns.map(([name, sig, doc]) => ({
        label: name,
        kind: CompletionItemKind.Function,
        detail: `${module}.${name}${sig}`,
        documentation: doc,
        insertText: `${name}(${sig.includes("()") ? "" : "${1}"})`,
        insertTextFormat: InsertTextFormat.Snippet,
    }));
}

export const TYPE_COMPLETIONS: CompletionItem[] = [
    "Int", "Decimal", "Str", "Bool", "Vec", "Map", "Set", "Func", "Nil",
].map(t => ({ label: t, kind: CompletionItemKind.Class, detail: "Primitive type" }));

// Hover documentation for keywords and common patterns.
export const HOVER_DOCS: Record<string, string> = {
    let: "`let name = expr;` binds a value. Variables are immutable by default; use `x = expr` to reassign.",
    fn: "`fn name(params) -> Type { body }` declares a function. Functions are first-class values.",
    if: "`if cond { then } else { else_ }` is an expression that returns a value.",
    match: "`match value { Pattern => expr, ... }` destructures and dispatches on patterns.",
    while: "`while cond { body }` loops while the condition holds. Returns Nil.",
    loop: "`loop { ... break value }` is an infinite loop; `break` returns a value.",
    spawn: "`spawn(initial) { body }` creates an actor with isolated state.",
    receive: "`receive { Pattern => handler }` blocks for a matching message.",
    shared: "`shared expr` creates a transactional cell for use in `transact`.",
    transact: "`transact { ... }` runs with snapshot isolation. Use `*cell` to read/write shared cells.",
    try: "`try { ... } rescue Pattern as e { ... }` catches exceptions raised by `raise`.",
    raise: "`raise expr` throws an exception (any Value).",
    import: "`import path;` loads a module. Use `lazy import` for deferred loading, `as` for aliasing.",
};
