// 1y language server — provides diagnostics, completions, hover, and
// document symbols over the LSP protocol.
//
// Diagnostics use a hybrid strategy:
//   1. A fast in-process TS lexer (lexer.ts) catches lexical issues
//      (unterminated strings, unmatched brackets) with zero latency.
//   2. If a `1y` executable is configured, `1y parse -` is spawned on
//      save/debounce to surface full parser diagnostics from the Rust
//      frontend, which is authoritative.
//
// Completions are context-aware: after `module.` we surface module
// functions; at the start of a statement we surface keywords; otherwise
// we surface builtins + identifiers in scope (best-effort).

import {
    createConnection,
    TextDocuments,
    Diagnostic,
    DiagnosticSeverity,
    ProposedFeatures,
    InitializeResult,
    ServerCapabilities,
    TextDocumentSyncKind,
    CompletionItem,
    CompletionItemKind,
    CompletionList,
    Hover,
    Position,
    Range,
    DocumentSymbol,
    SymbolKind,
} from "vscode-languageserver/node";

import { TextDocument } from "vscode-languageserver-textdocument";
import { execFile, ChildProcess } from "child_process";
import { tokenize, Token, TokenType, KEYWORDS } from "./lexer";
import {
    KEYWORD_COMPLETIONS,
    BUILTIN_COMPLETIONS,
    MODULE_COMPLETIONS,
    TYPE_COMPLETIONS,
    HOVER_DOCS,
} from "./completions";

// ---------------------------------------------------------------------------
// Connection & document manager
// ---------------------------------------------------------------------------

const connection = createConnection(ProposedFeatures.all);

const documents = new TextDocuments<TextDocument>(TextDocument);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

interface ServerSettings {
    executablePath: string;
    trace: "off" | "messages" | "verbose";
}

let settings: ServerSettings = {
    executablePath: "",
    trace: "off",
};

// Pending debounce timer for subprocess diagnostics.
let pendingParse: NodeJS.Timeout | null = null;
const PARSE_DEBOUNCE_MS = 300;

// Track pending child processes so we can kill them on re-parse.
let pendingChild: ChildProcess | null = null;

// ---------------------------------------------------------------------------
// Document analysis — lightweight symbol table for hover/completion
// ---------------------------------------------------------------------------

interface DocSymbol {
    name: string;
    kind: "fn" | "let" | "enum" | "type" | "actor" | "on" | "param";
    /** Source-text signature, e.g. `fn fact(n: Int) -> Int` or `let f500 = fact(500)`. */
    signature: string;
    /** Inferred type for `let` bindings (best-effort), e.g. "Int", "Str". */
    inferredType?: string;
    /** Character offset of the definition — used for scope filtering. */
    offset: number;
}

/** Per-URI symbol tables, refreshed on every document change. */
const docSymbols = new Map<string, DocSymbol[]>();

// ---------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------

connection.onInitialize((params) => {
    if (params.initializationOptions && typeof params.initializationOptions.executablePath === "string") {
        settings.executablePath = params.initializationOptions.executablePath;
    }
    const capabilities: ServerCapabilities = {
        textDocumentSync: TextDocumentSyncKind.Incremental,
        completionProvider: {
            resolveProvider: false,
            triggerCharacters: [".", ":", "(", "<"],
        },
        hoverProvider: true,
        documentSymbolProvider: true,
        definitionProvider: false,
    };
    const result: InitializeResult = { capabilities };
    return result;
});

connection.onInitialized(async () => {
    try {
        const config: any = await connection.workspace.getConfiguration("1y");
        if (config) {
            const exec: string | undefined = config.get("executablePath");
            const trace: "off" | "messages" | "verbose" = config.get("server.trace", "off");
            if (exec) settings.executablePath = exec;
            if (trace) settings.trace = trace;
        }
    } catch {
        // workspace configuration not available (e.g. in tests) — keep defaults
    }
    connection.console.info(`1y language server initialized (executable: ${settings.executablePath || "<none>"})`);
});

// Re-read configuration on change.
connection.onDidChangeConfiguration(async () => {
    try {
        const config: any = await connection.workspace.getConfiguration("1y");
        if (config) {
            const exec: string | undefined = config.get("executablePath");
            const trace: "off" | "messages" | "verbose" = config.get("server.trace", "off");
            if (exec !== undefined) settings.executablePath = exec;
            if (trace !== undefined) settings.trace = trace;
        }
    } catch {
        // ignore
    }
    // Revalidate all open documents.
    documents.all().forEach((doc) => validate(doc));
});

// ---------------------------------------------------------------------------
// Document events
// ---------------------------------------------------------------------------

documents.onDidChangeContent((event) => {
    validate(event.document);
});

documents.onDidClose((event) => {
    connection.sendDiagnostics({ uri: event.document.uri, diagnostics: [] });
    docSymbols.delete(event.document.uri);
});

// ---------------------------------------------------------------------------
// Diagnostics — fast lexical pass + optional subprocess parse
// ---------------------------------------------------------------------------

function validate(doc: TextDocument): void {
    // Always do the fast in-process pass for immediate feedback.
    const quickDiags = lexicalDiagnostics(doc);
    connection.sendDiagnostics({ uri: doc.uri, diagnostics: quickDiags });

    // Refresh the symbol table for hover/completion.
    docSymbols.set(doc.uri, analyzeDocument(doc.getText()));

    // If a `1y` executable is configured, also run the authoritative parser.
    if (settings.executablePath) {
        scheduleSubprocessParse(doc);
    }
}

/** Fast lexical diagnostics: unterminated strings, unmatched brackets. */
function lexicalDiagnostics(doc: TextDocument): Diagnostic[] {
    const src = doc.getText();
    const tokens = tokenize(src);
    const diags: Diagnostic[] = [];

    // 1. Unterminated strings.
    for (const tok of tokens) {
        if (tok.type === TokenType.StringStart) {
            diags.push({
                severity: DiagnosticSeverity.Error,
                range: tokenRange(doc, tok),
                message: "Unterminated string literal",
                source: "1y-lexer",
            });
        }
        if (tok.type === TokenType.Unknown) {
            diags.push({
                severity: DiagnosticSeverity.Warning,
                range: tokenRange(doc, tok),
                message: `Unexpected character ${JSON.stringify(tok.text)}`,
                source: "1y-lexer",
            });
        }
    }

    // 2. Bracket balance check.
    // HashBrace (`#{`) and EscLBrace (`\{`) both close with RBrace / EscRBrace.
    const closers: Record<number, number> = {
        [TokenType.LParen]: TokenType.RParen,
        [TokenType.LBrace]: TokenType.RBrace,
        [TokenType.LBracket]: TokenType.RBracket,
        [TokenType.HashBrace]: TokenType.RBrace,
        [TokenType.EscLBrace]: TokenType.EscRBrace,
    };
    const openerName: Record<number, string> = {
        [TokenType.LParen]: "(",
        [TokenType.LBrace]: "{",
        [TokenType.LBracket]: "[",
        [TokenType.HashBrace]: "#{",
        [TokenType.EscLBrace]: "\\{",
    };
    const closerName: Record<number, string> = {
        [TokenType.RParen]: ")",
        [TokenType.RBrace]: "}",
        [TokenType.RBracket]: "]",
        [TokenType.EscRBrace]: "\\}",
    };
    const stack: Token[] = [];
    for (const tok of tokens) {
        if (closers[tok.type]) {
            stack.push(tok);
        } else if (closerName[tok.type]) {
            // Check if the top of the stack is an opener whose expected
            // closer matches this token. This correctly handles many-to-one
            // mappings (e.g. both `{` and `#{` close with `}`).
            const top = stack[stack.length - 1];
            if (top && closers[top.type] === tok.type) {
                stack.pop();
            } else {
                diags.push({
                    severity: DiagnosticSeverity.Error,
                    range: tokenRange(doc, tok),
                    message: `Unmatched ${closerName[tok.type]}`,
                    source: "1y-lexer",
                });
            }
        }
    }
    // Any remaining openers are unclosed.
    for (const tok of stack) {
        diags.push({
            severity: DiagnosticSeverity.Error,
            range: tokenRange(doc, tok),
            message: `Unclosed ${openerName[tok.type]}`,
            source: "1y-lexer",
        });
    }

    return diags;
}

function tokenRange(doc: TextDocument, tok: Token): Range {
    const start = doc.positionAt(tok.start);
    const end = doc.positionAt(tok.end);
    return { start, end };
}

// ---------------------------------------------------------------------------
// Document analysis — extract user-defined symbols for hover
// ---------------------------------------------------------------------------

/** Scan token stream and extract all top-level + nested bindings. */
function analyzeDocument(src: string): DocSymbol[] {
    const tokens = tokenize(src);
    const symbols: DocSymbol[] = [];

    for (let i = 0; i < tokens.length; i++) {
        const tok = tokens[i];
        if (tok.type !== TokenType.Keyword) continue;

        const kw = tok.text;
        if (kw !== "fn" && kw !== "let" && kw !== "enum" && kw !== "type" && kw !== "actor" && kw !== "on") continue;

        const next = tokens[i + 1];
        if (!next || next.type !== TokenType.Ident) continue;

        const name = next.text;
        const offset = tok.start;

        if (kw === "fn") {
            // Signature: from `fn` to `{` (exclusive) or end of line.
            let sigEnd = next.end;
            let parenDepth = 0;
            for (let j = i + 2; j < tokens.length; j++) {
                const t = tokens[j];
                if (t.type === TokenType.LParen) parenDepth++;
                else if (t.type === TokenType.RParen) parenDepth--;
                else if (t.type === TokenType.LBrace && parenDepth === 0) { sigEnd = t.start; break; }
                else if (t.type === TokenType.Semicolon || (t.line !== tok.line && parenDepth === 0)) { sigEnd = t.start; break; }
                sigEnd = t.end;
            }
            const signature = src.substring(tok.start, sigEnd).trim();
            symbols.push({ name, kind: "fn", signature, offset });

            // Extract parameters for param hover.
            extractParams(src, tokens, i, next, symbols);
        } else if (kw === "let") {
            // Signature: from `let` to `;` or end of line.
            let sigEnd = next.end;
            let rhsStart = -1;
            for (let j = i + 2; j < tokens.length; j++) {
                const t = tokens[j];
                if (t.type === TokenType.Semicolon || t.line !== tok.line) { sigEnd = t.type === TokenType.Semicolon ? t.start : sigEnd; break; }
                if (t.type === TokenType.Assign && rhsStart < 0) rhsStart = tokens[j + 1] ? tokens[j + 1].start : -1;
                sigEnd = t.end;
            }
            const signature = src.substring(tok.start, sigEnd).trim();
            const inferredType = rhsStart >= 0 ? inferType(src, rhsStart) : undefined;
            symbols.push({ name, kind: "let", signature, inferredType, offset });
        } else if (kw === "enum" || kw === "type" || kw === "actor") {
            // Signature: from keyword to matching `}`.
            let braceIdx = -1;
            for (let j = i + 2; j < tokens.length; j++) {
                if (tokens[j].type === TokenType.LBrace) { braceIdx = j; break; }
                if (tokens[j].type === TokenType.Semicolon || tokens[j].line !== tok.line) break;
            }
            if (braceIdx >= 0) {
                let depth = 1;
                let endIdx = braceIdx + 1;
                while (endIdx < tokens.length && depth > 0) {
                    if (tokens[endIdx].type === TokenType.LBrace) depth++;
                    else if (tokens[endIdx].type === TokenType.RBrace) depth--;
                    endIdx++;
                }
                const signature = src.substring(tok.start, tokens[endIdx - 1].end).trim();
                symbols.push({ name, kind: kw as DocSymbol["kind"], signature, offset });
            } else {
                const signature = src.substring(tok.start, next.end).trim();
                symbols.push({ name, kind: kw as DocSymbol["kind"], signature, offset });
            }
        } else if (kw === "on") {
            // Signature: from `on` to `{` (exclusive).
            let sigEnd = next.end;
            for (let j = i + 2; j < tokens.length; j++) {
                if (tokens[j].type === TokenType.LBrace) { sigEnd = tokens[j].start; break; }
                sigEnd = tokens[j].end;
            }
            const signature = src.substring(tok.start, sigEnd).trim();
            symbols.push({ name, kind: "on", signature, offset });
        }
    }

    return symbols;
}

/** Extract function parameters and add them as param symbols. */
function extractParams(
    _src: string,
    tokens: Token[],
    fnIdx: number,
    fnNameTok: Token,
    symbols: DocSymbol[],
): void {
    // Find `(` after the function name.
    const lparen = tokens[fnIdx + 2];
    if (!lparen || lparen.type !== TokenType.LParen) return;

    let j = fnIdx + 3;
    let depth = 1;
    let paramName: string | null = null;
    let paramSigStart = j;

    while (j < tokens.length && depth > 0) {
        const t = tokens[j];
        if (t.type === TokenType.LParen) depth++;
        else if (t.type === TokenType.RParen) {
            depth--;
            if (depth === 0) {
                // Last parameter before `)`.
                if (paramName) {
                    const sig = _src.substring(tokens[paramSigStart].start, t.start).trim();
                    symbols.push({ name: paramName, kind: "param", signature: sig, offset: fnNameTok.start });
                }
                break;
            }
        } else if (depth === 1) {
            if (t.type === TokenType.Ident && paramName === null) {
                paramName = t.text;
            } else if (t.type === TokenType.Comma) {
                if (paramName) {
                    const sig = _src.substring(tokens[paramSigStart].start, t.start).trim();
                    symbols.push({ name: paramName, kind: "param", signature: sig, offset: fnNameTok.start });
                }
                paramName = null;
                paramSigStart = j + 1;
            }
        }
        j++;
    }
}

/** Best-effort type inference from the RHS expression's first token. */
function inferType(src: string, rhsOffset: number): string | undefined {
    const tokens = tokenize(src);
    // Find the first non-whitespace token at or after rhsOffset.
    for (const t of tokens) {
        if (t.start >= rhsOffset) {
            if (t.type === TokenType.Int) return "Int";
            if (t.type === TokenType.Decimal) return "Decimal";
            if (t.type === TokenType.String) return "Str";
            if (t.type === TokenType.Keyword && (t.text === "true" || t.text === "false")) return "Bool";
            if (t.type === TokenType.Keyword && t.text === "nil") return "Nil";
            if (t.type === TokenType.LBracket) return "Vec";
            if (t.type === TokenType.LBrace) return "Map";
            if (t.type === TokenType.Ident) return undefined; // can't infer
            return undefined;
        }
    }
    return undefined;
}

/** Find the closest preceding symbol with a matching name (scope-aware). */
function findUserSymbol(uri: string, name: string, cursorOffset: number): DocSymbol | null {
    const symbols = docSymbols.get(uri);
    if (!symbols) return null;
    let best: DocSymbol | null = null;
    for (const sym of symbols) {
        if (sym.name === name && sym.offset <= cursorOffset) {
            if (!best || sym.offset > best.offset) {
                best = sym;
            }
        }
    }
    return best;
}

/** Debounced subprocess invocation of `1y parse -` for authoritative diagnostics. */
function scheduleSubprocessParse(doc: TextDocument): void {
    if (pendingParse) clearTimeout(pendingParse);
    pendingParse = setTimeout(() => {
        pendingParse = null;
        runSubprocessParse(doc).catch((err) => {
            connection.console.warn(`subprocess parse failed: ${err}`);
        });
    }, PARSE_DEBOUNCE_MS);
}

async function runSubprocessParse(doc: TextDocument): Promise<void> {
    const src = doc.getText();
    // Kill any previous pending parse — we only care about the latest.
    if (pendingChild) {
        try { pendingChild.kill(); } catch { /* ignore */ }
        pendingChild = null;
    }

    return new Promise((resolve) => {
        const child = execFile(
            settings.executablePath,
            ["parse", "-"],
            {
                maxBuffer: 16 * 1024 * 1024,
                timeout: 5000,
                windowsHide: true,
            },
            (err, _stdout, stderr) => {
                if (pendingChild === child) pendingChild = null;
                // Non-zero exit is expected when there are parse errors.
                // The actual diagnostics are on stderr.
                const diags = parseRustErrors(stderr || "", doc);
                connection.sendDiagnostics({ uri: doc.uri, diagnostics: diags });
                resolve();
            },
        );
        pendingChild = child;
        // Pipe source to stdin.
        if (child.stdin) {
            child.stdin.on("error", () => { /* swallow EPIPE if child exited early */ });
            child.stdin.end(src, "utf-8");
        }
        // If spawn itself failed (ENOENT), surface a one-shot warning.
        child.on("error", (e: NodeJS.ErrnoException) => {
            if (e.code === "ENOENT") {
                connection.console.warn(
                    `1y executable not found at ${settings.executablePath}; ` +
                    `disabling subprocess diagnostics. Set "1y.executablePath" to a valid path.`,
                );
                settings.executablePath = ""; // disable further attempts
            } else {
                connection.console.warn(`1y parse subprocess error: ${e.message}`);
            }
            resolve();
        });
    });
}

/**
 * Parse `1y parse` stderr output into LSP diagnostics.
 *
 * The Rust renderer produces blocks of the form:
 *
 *     error: <message>
 *       --> L:C-L:C
 *      | <source line>
 *      |    ^^^
 *      = hint: <hint>
 *
 * We extract the `error:` line (for message) and the `-->` line (for span).
 * Multiple errors are separated by blank lines.
 */
function parseRustErrors(stderr: string, doc: TextDocument): Diagnostic[] {
    const diags: Diagnostic[] = [];
    const lines = stderr.split(/\r?\n/);
    let i = 0;
    while (i < lines.length) {
        const line = lines[i];
        const m = /^error:\s*(.*)$/.exec(line);
        if (m) {
            const message = m[1];
            // Look ahead for the span line `  --> L:C-L:C`.
            let range: Range | null = null;
            for (let j = i + 1; j < Math.min(i + 4, lines.length); j++) {
                const sm = /^\s*-->\s*(\d+):(\d+)\s*-\s*(\d+):(\d+)/.exec(lines[j]);
                if (sm) {
                    const startLine = parseInt(sm[1], 10) - 1;
                    const startCol = parseInt(sm[2], 10) - 1;
                    const endLine = parseInt(sm[3], 10) - 1;
                    const endCol = parseInt(sm[4], 10);
                    range = {
                        start: Position.create(startLine, startCol),
                        end: Position.create(endLine, endCol),
                    };
                    break;
                }
            }
            if (!range) {
                // No span — put it at the top of the file.
                range = Range.create(Position.create(0, 0), Position.create(0, 1));
            }
            diags.push({
                severity: DiagnosticSeverity.Error,
                range,
                message,
                source: "1y",
            });
            // Advance past the block we've consumed (message + span + underline + hint).
            i += 4;
            continue;
        }
        i++;
    }
    return diags;
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

connection.onCompletion((params): CompletionList | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;

    const pos = params.position;
    const offset = doc.offsetAt(pos);
    const src = doc.getText();

    // Identify the identifier/prefix being typed.
    let start = offset;
    while (start > 0) {
        const c = src[start - 1];
        if (/[A-Za-z0-9_]/.test(c)) start--;
        else break;
    }
    const prefix = src.substring(start, offset);

    // Detect `module.` context — look back for `<ident>.`
    let moduleContext: string | null = null;
    if (start >= 2 && src[start - 1] === ".") {
        let mEnd = start - 1;
        let mStart = mEnd;
        while (mStart > 0 && /[A-Za-z0-9_]/.test(src[mStart - 1])) mStart--;
        const candidate = src.substring(mStart, mEnd);
        if (candidate && MODULE_COMPLETIONS[candidate]) {
            moduleContext = candidate;
        }
    }

    if (moduleContext) {
        const items = MODULE_COMPLETIONS[moduleContext];
        // Filter by prefix (after the dot).
        const filtered = prefix
            ? items.filter((it) => it.label.startsWith(prefix))
            : items;
        return { isIncomplete: false, items: filtered };
    }

    // Detect `import <prefix>` context — only module names.
    const lineUpToCursor = src.substring(
        doc.offsetAt(Position.create(pos.line, 0)),
        offset,
    );
    const importMatch = /^\s*(lazy\s+)?import\s+(\w*)$/.exec(lineUpToCursor);
    if (importMatch) {
        const modItems: CompletionItem[] = Object.keys(MODULE_COMPLETIONS).map((name) => ({
            label: name,
            kind: CompletionItemKind.Module,
            detail: `module ${name}`,
            documentation: `Standard library module: ${name}`,
        }));
        const modPrefix = importMatch[2];
        const filtered = modPrefix
            ? modItems.filter((it) => it.label.startsWith(modPrefix))
            : modItems;
        return { isIncomplete: false, items: filtered };
    }

    // Detect type-annotation context — after `:` `->`, or in type position.
    const colonPrefix = /^\s*(:|->)\s*\w*$/.exec(lineUpToCursor);
    if (colonPrefix) {
        return {
            isIncomplete: false,
            items: prefix
                ? TYPE_COMPLETIONS.filter((t) => t.label.startsWith(prefix))
                : TYPE_COMPLETIONS,
        };
    }

    // Default: keywords (only at statement start) + builtins + types.
    const items: CompletionItem[] = [];

    // At statement start (only whitespace before), offer keywords.
    if (/^\s*$/.test(lineUpToCursor)) {
        items.push(...KEYWORD_COMPLETIONS);
    }

    // Builtins — filtered by prefix.
    const builtins = prefix
        ? BUILTIN_COMPLETIONS.filter((b) => b.label.startsWith(prefix))
        : BUILTIN_COMPLETIONS;
    items.push(...builtins);

    // Types — useful at expression positions too.
    const types = prefix
        ? TYPE_COMPLETIONS.filter((t) => t.label.startsWith(prefix))
        : TYPE_COMPLETIONS;
    items.push(...types);

    // If prefix is empty and we're not at statement start, still offer keywords.
    if (!prefix && !/^\s*$/.test(lineUpToCursor)) {
        items.push(...KEYWORD_COMPLETIONS);
    }

    return { isIncomplete: false, items };
});

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

connection.onHover((params): Hover | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;

    const pos = params.position;
    const offset = doc.offsetAt(pos);
    const src = doc.getText();

    // Find the identifier under the cursor.
    let start = offset;
    while (start > 0 && /[A-Za-z0-9_]/.test(src[start - 1])) start--;
    let end = offset;
    while (end < src.length && /[A-Za-z0-9_]/.test(src[end])) end++;
    const word = src.substring(start, end);
    if (!word) return null;

    // Keyword hover docs.
    if (KEYWORDS.has(word) && HOVER_DOCS[word]) {
        return {
            contents: {
                kind: "markdown",
                value: HOVER_DOCS[word],
            },
            range: {
                start: doc.positionAt(start),
                end: doc.positionAt(end),
            },
        };
    }

    // Builtin function hover — find by label.
    const builtin = BUILTIN_COMPLETIONS.find((b) => b.label === word);
    if (builtin) {
        const detail = builtin.detail || "";
        const doc_text = builtin.documentation
            ? typeof builtin.documentation === "string"
                ? builtin.documentation
                : (builtin.documentation as any).value || ""
            : "";
        return {
            contents: {
                kind: "markdown",
                value: `\`${word}${detail}\`\n\n${doc_text}`,
            },
            range: {
                start: doc.positionAt(start),
                end: doc.positionAt(end),
            },
        };
    }

    // Module function hover — for `module.func`, look back.
    if (start >= 2 && src[start - 1] === ".") {
        let mEnd = start - 1;
        let mStart = mEnd;
        while (mStart > 0 && /[A-Za-z0-9_]/.test(src[mStart - 1])) mStart--;
        const modName = src.substring(mStart, mEnd);
        const modItems = MODULE_COMPLETIONS[modName];
        if (modItems) {
            const fnItem = modItems.find((it) => it.label === word);
            if (fnItem) {
                const detail = fnItem.detail || "";
                const doc_text = fnItem.documentation
                    ? typeof fnItem.documentation === "string"
                        ? fnItem.documentation
                        : (fnItem.documentation as any).value || ""
                    : "";
                return {
                    contents: {
                        kind: "markdown",
                        value: `\`${modName}.${word}${detail}\`\n\n${doc_text}`,
                    },
                    range: {
                        start: doc.positionAt(start),
                        end: doc.positionAt(end),
                    },
                };
            }
        }
    }

    // User-defined symbol hover — variables, functions, params, types.
    const sym = findUserSymbol(doc.uri, word, start);
    if (sym) {
        const kindLabel =
            sym.kind === "fn" ? "Function" :
            sym.kind === "let" ? "Binding" :
            sym.kind === "enum" ? "Enum" :
            sym.kind === "type" ? "Type" :
            sym.kind === "actor" ? "Actor" :
            sym.kind === "on" ? "Handler" :
            "Parameter";
        const typeHint = sym.inferredType ? `  \n\n**Type:** \`${sym.inferredType}\`` : "";
        return {
            contents: {
                kind: "markdown",
                value: `**${kindLabel}**${typeHint}\n\n\`\`\`1y\n${sym.signature}\n\`\`\``,
            },
            range: {
                start: doc.positionAt(start),
                end: doc.positionAt(end),
            },
        };
    }

    return null;
});

// ---------------------------------------------------------------------------
// Document symbols — a lightweight scan for top-level declarations
// ---------------------------------------------------------------------------

connection.onDocumentSymbol((params): DocumentSymbol[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];

    const src = doc.getText();
    const tokens = tokenize(src);
    const symbols: DocumentSymbol[] = [];

    // Walk top-level (depth 0) declarations: `fn`, `let`, `enum`, `type`, `actor`, `on`.
    let depth = 0;
    for (let i = 0; i < tokens.length; i++) {
        const tok = tokens[i];
        switch (tok.type) {
            case TokenType.LBrace:
            case TokenType.LParen:
            case TokenType.LBracket:
                depth++;
                break;
            case TokenType.RBrace:
            case TokenType.RParen:
            case TokenType.RBracket:
                depth = Math.max(0, depth - 1);
                break;
        }
        if (depth !== 0) continue;
        if (tok.type !== TokenType.Keyword) continue;

        const kw = tok.text;
        // `fn name`, `let name`, `enum Name`, `type Name`, `actor Name`, `on name`
        if (kw === "fn" || kw === "let" || kw === "enum" || kw === "type" || kw === "actor" || kw === "on") {
            const next = tokens[i + 1];
            if (next && next.type === TokenType.Ident) {
                const symbolKind =
                    kw === "fn" ? SymbolKind.Function :
                    kw === "let" ? SymbolKind.Variable :
                    kw === "enum" ? SymbolKind.Enum :
                    kw === "type" ? SymbolKind.Struct :
                    kw === "actor" ? SymbolKind.Class :
                    SymbolKind.Method; // `on` handler
                symbols.push({
                    name: next.text,
                    kind: symbolKind,
                    // range must contain selectionRange — span from keyword to ident end.
                    range: {
                        start: doc.positionAt(tok.start),
                        end: doc.positionAt(next.end),
                    },
                    selectionRange: tokenRange(doc, next),
                });
            }
        }
    }
    return symbols;
});

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

connection.onShutdown(() => {
    if (pendingChild) {
        try { pendingChild.kill(); } catch { /* ignore */ }
        pendingChild = null;
    }
    if (pendingParse) {
        clearTimeout(pendingParse);
        pendingParse = null;
    }
});

// ---------------------------------------------------------------------------
// Listen
// ---------------------------------------------------------------------------

documents.listen(connection);
connection.listen();
