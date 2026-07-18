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
    Definition,
    SignatureHelp,
    ParameterInformation,
    Location,
    DocumentHighlight,
    DocumentHighlightKind,
    WorkspaceEdit,
    TextEdit,
    WorkspaceSymbol,
    SemanticTokens,
    SemanticTokensRegistrationType,
    InlayHint as LspInlayHint,
    FoldingRange,
    SelectionRange,
    CodeLens,
    DocumentLink,
} from "vscode-languageserver/node";

import { TextDocument } from "vscode-languageserver-textdocument";
import { execFile, ChildProcess } from "child_process";
import { tokenize, Token, TokenType, KEYWORDS } from "./lexer";
import { parse } from "./parser";
import { attachDocStrings } from "./docstrings";
import {
    buildSymbolTable, resolveAt, visibleAt, declAt, identAt,
    SymbolTable, SymbolInfo, SymbolKind as SymKind,
} from "./symbols";
import { buildTypeMap, symbolType, typeToText, TypeMap } from "./types";
import {
    classifySemanticTokens,
    SEMANTIC_TOKEN_LEGEND,
} from "./semanticTokens";
import { buildInlayHints } from "./inlayHints";
import { buildFoldingRanges, buildCommentAndImportFolds } from "./foldingRange";
import { buildSelectionRange } from "./selectionRange";
import {
    buildWorkspaceIndex, refreshFile, removeFile, resolveModuleUri,
    findSymbolInFile, findSymbolAnywhere, searchWorkspaceSymbols,
    WorkspaceIndex, SymKind as WsSymKind,
} from "./workspaceIndex";
import { Program, annotToText, paramsToText, ParseError } from "./ast";
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
// Document analysis — AST + symbol table + type map, cached per URI
// ---------------------------------------------------------------------------

interface DocAnalysis {
    src: string;
    program: Program;
    errors: ParseError[];
    table: SymbolTable;
    typeMap: TypeMap;
    tokens: Token[];
}

/** Per-URI analysis cache, refreshed on every document change. */
const docAnalysis = new Map<string, DocAnalysis>();

// Workspace-wide index for cross-file definition + workspace symbol search.
// Built lazily on first use (some clients send no workspace folders).
let workspaceIndex: WorkspaceIndex | null = null;

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
        definitionProvider: true,
        signatureHelpProvider: {
            triggerCharacters: ["(", ","],
            retriggerCharacters: [","],
        },
        referencesProvider: true,
        documentHighlightProvider: true,
        renameProvider: true,
        workspaceSymbolProvider: true,
        semanticTokensProvider: {
            legend: {
                tokenTypes: SEMANTIC_TOKEN_LEGEND.tokenTypes as unknown as string[],
                tokenModifiers: SEMANTIC_TOKEN_LEGEND.tokenModifiers as unknown as string[],
            },
            full: true,
            range: false,
        },
        inlayHintProvider: true,
        foldingRangeProvider: true,
        selectionRangeProvider: true,
        typeDefinitionProvider: true,
        codeLensProvider: { resolveProvider: false },
        documentLinkProvider: { resolveProvider: false },
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

    // Build the workspace index from workspace folders, for cross-file
    // definition and Ctrl+T symbol search. Lazily created if absent.
    try {
        const folders = await connection.workspace.getWorkspaceFolders();
        if (folders && folders.length > 0) {
            const roots = folders.map((f) => fileUriToPath(f.uri));
            workspaceIndex = buildWorkspaceIndex(roots);
            connection.console.info(`1y workspace index: ${workspaceIndex.files.size} files, ${workspaceIndex.symbols.length} symbols`);
        }
    } catch {
        // no workspace access — cross-file features degrade to single-file.
    }

    connection.console.info(`1y language server initialized (executable: ${settings.executablePath || "<none>"})`);
});

/** Convert a `file://` URI to an OS path. Used for workspace folder roots. */
function fileUriToPath(uri: string): string {
    const m = /^file:\/\/\/(.*)$/.exec(uri);
    if (!m) return uri;
    let p = decodeURIComponent(m[1]);
    if (!/^[A-Za-z]:[\\/]/.test(p)) p = "/" + p;
    return p;
}

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
    docAnalysis.delete(event.document.uri);
    if (workspaceIndex) removeFile(workspaceIndex, event.document.uri);
});

// ---------------------------------------------------------------------------
// Diagnostics — fast lexical pass + AST parse errors + optional subprocess
// ---------------------------------------------------------------------------

function validate(doc: TextDocument): void {
    const src = doc.getText();

    // Refresh the AST analysis cache (used by hover/completion/definition).
    const analysis = analyzeDocument(src);
    docAnalysis.set(doc.uri, analysis);

    // Keep the workspace index in sync for cross-file features.
    if (workspaceIndex) refreshFile(workspaceIndex, doc.uri, src);

    // Lexical diagnostics (unterminated strings, unmatched brackets).
    const diags = lexicalDiagnostics(doc, analysis.tokens);

    // AST parse-error diagnostics from the in-process TS parser.
    for (const e of analysis.errors) {
        diags.push({
            severity: DiagnosticSeverity.Error,
            range: {
                start: doc.positionAt(e.span.start),
                end: doc.positionAt(e.span.end),
            },
            message: e.message,
            source: "1y-parser",
        });
    }
    connection.sendDiagnostics({ uri: doc.uri, diagnostics: diags });

    // If a `1y` executable is configured, also run the authoritative parser.
    if (settings.executablePath) {
        scheduleSubprocessParse(doc);
    }
}

/** Fast lexical diagnostics: unterminated strings, unmatched brackets. */
function lexicalDiagnostics(doc: TextDocument, tokens: Token[]): Diagnostic[] {
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
// Document analysis — build AST, symbol table, and type map
// ---------------------------------------------------------------------------

/** Parse source into the cached analysis (AST + symbols + types + tokens). */
function analyzeDocument(src: string): DocAnalysis {
    const tokens = tokenize(src);
    const r = parse(src);
    attachDocStrings(src, r.program);
    const table = buildSymbolTable(src, r.program);
    const typeMap = buildTypeMap(r.program, table);
    return { src, program: r.program, errors: r.errors, table, typeMap, tokens };
}

/**
 * Check if `offset` falls inside a string literal or comment token. When
 * true, hover/definition/completion etc. should not fire — the text under
 * the cursor is not an identifier but string/comment content.
 *
 * Interpolation inside strings is tokenized as part of the String token by
 * our lexer (naive treatment), so the entire `"...${expr}..."` is one token
 * and any position inside it returns true here.
 */
function isInStringOrComment(tokens: Token[], offset: number): boolean {
    for (const t of tokens) {
        if (t.type === TokenType.String
            || t.type === TokenType.Comment
            || t.type === TokenType.DocComment) {
            if (offset >= t.start && offset < t.end) return true;
        }
    }
    return false;
}

// ---------------------------------------------------------------------------
// Symbol rendering helpers — build hover/signature text from a SymbolInfo
// ---------------------------------------------------------------------------

/** Render a SymbolInfo's signature (source-like text without the body). */
function symbolSignature(sym: SymbolInfo, src: string): string {
    return src.substring(sym.declSpan.start, sym.declSpan.end).trim();
}

/** Human-readable kind label for hover. */
function kindLabel(kind: SymKind): string {
    switch (kind) {
        case "function": return "Function";
        case "on": return "Handler";
        case "let": return "Binding";
        case "shared": return "Shared Binding";
        case "state": return "Actor State";
        case "param": return "Parameter";
        case "lambda_param": return "Lambda Parameter";
        case "type": return "Type";
        case "enum": return "Enum";
        case "actor": return "Actor";
        case "variant": return "Variant";
        case "import": return "Import";
        case "for_var": return "Loop Variable";
        case "match_bind": return "Match Binding";
    }
}

/** Build a markdown hover string for a user-defined symbol. */
function symbolHoverMarkdown(sym: SymbolInfo, src: string, typeMap: TypeMap): string {
    const label = kindLabel(sym.kind);
    const sig = symbolSignature(sym, src);
    const t = symbolType(sym, typeMap);
    const typeText = t.kind !== "Unknown" ? typeToText(t) : "";
    const parts: string[] = [];
    parts.push(`**${label}**`);
    if (typeText) parts.push(`  \n**Type:** \`${typeText}\``);
    if (sym.doc) parts.push(`\n${sym.doc}`);
    parts.push(`\n\`\`\`1y\n${sig}\n\`\`\``);
    return parts.join("\n");
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

    // Don't complete inside strings — the user is typing string content,
    // not an identifier. Comments are left alone too.
    const analysis = docAnalysis.get(doc.uri);
    if (analysis && isInStringOrComment(analysis.tokens, offset)) {
        return { isIncomplete: false, items: [] };
    }

    // Identify the identifier/prefix being typed.
    let start = offset;
    while (start > 0) {
        const c = src[start - 1];
        if (/[A-Za-z0-9_]/.test(c)) start--;
        else break;
    }
    const prefix = src.substring(start, offset);

    // Detect `module.` or `alias.` context — look back for `<ident>.`
    let moduleContext: string | null = null;
    if (start >= 2 && src[start - 1] === ".") {
        const analysis = docAnalysis.get(doc.uri);
        moduleContext = resolveModulePrefix(src, start, analysis);
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
        const typeItems: CompletionItem[] = prefix
            ? TYPE_COMPLETIONS.filter((t) => t.label.startsWith(prefix))
            : TYPE_COMPLETIONS.slice();
        // Also list user-defined types / enums / actors in scope so the user
        // can annotate `let p: Point` without typing the whole name.
        const analysis = docAnalysis.get(doc.uri);
        if (analysis) {
            const seen = new Set(typeItems.map((t) => t.label));
            for (const sym of visibleAt(analysis.table, offset)) {
                if (seen.has(sym.name)) continue;
                if (sym.kind !== "type" && sym.kind !== "enum" && sym.kind !== "actor") continue;
                if (prefix && !sym.name.startsWith(prefix)) continue;
                seen.add(sym.name);
                typeItems.push({
                    label: sym.name,
                    kind: sym.kind === "type" ? CompletionItemKind.Struct
                        : sym.kind === "enum" ? CompletionItemKind.Enum
                        : CompletionItemKind.Class,
                    detail: sym.kind,
                    documentation: sym.doc ? { kind: "markdown", value: sym.doc } : undefined,
                });
            }
        }
        return { isIncomplete: false, items: typeItems };
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

    // In-scope user-defined identifiers (functions, lets, params, types...).
    if (analysis) {
        const seen = new Set<string>();
        for (const it of items) seen.add(it.label);
        const visible = visibleAt(analysis.table, offset);
        for (const sym of visible) {
            if (seen.has(sym.name)) continue;
            if (prefix && !sym.name.startsWith(prefix)) continue;
            seen.add(sym.name);
            const ck = completionKindFor(sym.kind);
            const t = symbolType(sym, analysis.typeMap);
            const detail = t.kind !== "Unknown" ? typeToText(t) : undefined;
            // For functions / handlers with params, generate a snippet so
            // accepting the completion inserts `(param1, param2)` with
            // tab-stops. This mirrors the behavior of TS/Rust extensions.
            let insertText: string | undefined;
            let insertTextFormat: 2 | undefined;
            if ((sym.kind === "function" || sym.kind === "on") && sym.params && sym.params.length > 0) {
                const placeholders = sym.params.map((p, i) =>
                    `\${${i + 1}:${p.name}}`);
                insertText = `${sym.name}(${placeholders.join(", ")})`;
                insertTextFormat = 2; // Snippet
            } else if ((sym.kind === "function" || sym.kind === "on") && (!sym.params || sym.params.length === 0)) {
                insertText = `${sym.name}()`;
                insertTextFormat = 2;
            }
            items.push({
                label: sym.name,
                kind: ck,
                detail,
                documentation: sym.doc ? { kind: "markdown", value: sym.doc } : undefined,
                insertText,
                insertTextFormat,
            });
        }
    }

    // If prefix is empty and we're not at statement start, still offer keywords.
    if (!prefix && !/^\s*$/.test(lineUpToCursor)) {
        items.push(...KEYWORD_COMPLETIONS);
    }

    return { isIncomplete: false, items };
});

/** Map a 1y SymbolKind to an LSP CompletionItemKind. */
function completionKindFor(k: SymKind): CompletionItemKind {
    switch (k) {
        case "function": return CompletionItemKind.Function;
        case "on": return CompletionItemKind.Method;
        case "let": case "shared": case "state": return CompletionItemKind.Variable;
        case "param": case "lambda_param": case "for_var": case "match_bind":
            return CompletionItemKind.Variable;
        case "type": return CompletionItemKind.Struct;
        case "enum": return CompletionItemKind.Enum;
        case "variant": return CompletionItemKind.EnumMember;
        case "actor": return CompletionItemKind.Class;
        case "import": return CompletionItemKind.Module;
    }
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

connection.onHover((params): Hover | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;

    const pos = params.position;
    const offset = doc.offsetAt(pos);
    const src = doc.getText();
    const analysis = docAnalysis.get(doc.uri);

    // Don't hover inside strings or comments — the text there isn't an
    // identifier, even if it happens to look like one.
    if (analysis && isInStringOrComment(analysis.tokens, offset)) return null;

    // Find the identifier under the cursor.
    let start = offset;
    while (start > 0 && /[A-Za-z0-9_]/.test(src[start - 1])) start--;
    let end = offset;
    while (end < src.length && /[A-Za-z0-9_]/.test(src[end])) end++;
    const word = src.substring(start, end);
    if (!word) return null;

    const hoverRange = {
        start: doc.positionAt(start),
        end: doc.positionAt(end),
    };

    // 1. Keyword hover docs — keywords are reserved, never shadowed.
    if (KEYWORDS.has(word) && HOVER_DOCS[word]) {
        return { contents: { kind: "markdown", value: HOVER_DOCS[word] }, range: hoverRange };
    }

    // 2. User-defined symbol hover (resolves first so user definitions
    //    shadow builtins of the same name, e.g. a user `count` function).
    //    Use resolveAt for use-sites; fall back to declAt so hovering on a
    //    declaration's own name also works.
    if (analysis) {
        let sym = resolveAt(analysis.table, start, word);
        if (!sym) {
            // Cursor might be on the declaration name itself (where
            // visibleFrom hasn't been reached yet).
            const onDecl = declAt(analysis.table, start);
            if (onDecl && onDecl.name === word) sym = onDecl;
        }
        if (sym) {
            // Import alias — show which module it aliases.
            if (sym.kind === "import" && sym.modulePath) {
                const aliasNote = sym.name !== sym.modulePath
                    ? `  \n**Alias of:** \`${sym.modulePath}\``
                    : "";
                return {
                    contents: {
                        kind: "markdown",
                        value: `**Module Import**${aliasNote}\n\n\`import ${sym.modulePath}${sym.name !== (sym.modulePath.split(".").pop() || sym.modulePath) ? " as " + sym.name : ""}\``,
                    },
                    range: hoverRange,
                };
            }
            return {
                contents: {
                    kind: "markdown",
                    value: symbolHoverMarkdown(sym, analysis.src, analysis.typeMap),
                },
                range: hoverRange,
            };
        }
    }

    // 3. `module.func` or `alias.func` hover — for field access on a module
    //    name or import alias. Resolve the prefix to a module name, then
    //    look up the function in MODULE_COMPLETIONS.
    if (start >= 2 && src[start - 1] === ".") {
        const modName = resolveModulePrefix(src, start, analysis);
        if (modName) {
            const modItems = MODULE_COMPLETIONS[modName];
            const fnItem = modItems ? modItems.find((it) => it.label === word) : undefined;
            if (fnItem) {
                const detail = fnItem.detail || "";
                const doc_text = docTextOf(fnItem);
                return {
                    contents: {
                        kind: "markdown",
                        value: `\`${modName}.${word}${detail}\`\n\n${doc_text}`,
                    },
                    range: hoverRange,
                };
            }
        }
    }

    // 4. Builtin function hover — only if no user-defined symbol matched.
    const builtin = BUILTIN_COMPLETIONS.find((b) => b.label === word);
    if (builtin) {
        const detail = builtin.detail || "";
        const doc_text = docTextOf(builtin);
        return {
            contents: { kind: "markdown", value: `\`${word}${detail}\`\n\n${doc_text}` },
            range: hoverRange,
        };
    }

    return null;
});

/** Extract the doc-text string from a CompletionItem's documentation field. */
function docTextOf(item: { documentation?: string | { value?: string } }): string {
    if (!item.documentation) return "";
    return typeof item.documentation === "string"
        ? item.documentation
        : item.documentation.value || "";
}

/**
 * Given source and the offset just after a `.`, resolve the identifier prefix
 * before the dot to a module name. Handles both direct module names (`json.`)
 * and import aliases (`J.` where `import json as J`). Returns the canonical
 * module name (e.g. "json"), or null if the prefix isn't a known module.
 */
function resolveModulePrefix(
    src: string,
    afterDotOffset: number,
    analysis: DocAnalysis | undefined,
): string | null {
    let mEnd = afterDotOffset - 1; // the dot
    let mStart = mEnd;
    while (mStart > 0 && /[A-Za-z0-9_]/.test(src[mStart - 1])) mStart--;
    const candidate = src.substring(mStart, mEnd);
    if (!candidate) return null;

    // Direct module name.
    if (MODULE_COMPLETIONS[candidate]) return candidate;

    // Import alias — resolve via the symbol table.
    if (analysis) {
        const sym = resolveAt(analysis.table, mStart, candidate)
            ?? (analysis.table.decls.find(d => d.name === candidate && d.kind === "import") || null);
        if (sym && sym.kind === "import" && sym.modulePath) {
            const modName = sym.modulePath.split(".").pop() || sym.modulePath;
            if (MODULE_COMPLETIONS[modName]) return modName;
        }
    }
    return null;
}

// ---------------------------------------------------------------------------
// Document symbols — top-level declarations from the AST symbol table
// ---------------------------------------------------------------------------

connection.onDocumentSymbol((params): DocumentSymbol[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];

    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const symbols: DocumentSymbol[] = [];
    // Only top-level (global scope) declarations appear in the outline.
    for (const sym of analysis.table.decls) {
        if (sym.scopeStart !== 0) continue;
        symbols.push({
            name: sym.name,
            kind: docSymbolKindFor(sym.kind),
            range: {
                start: doc.positionAt(sym.declSpan.start),
                end: doc.positionAt(sym.declSpan.end),
            },
            selectionRange: {
                start: doc.positionAt(sym.nameSpan.start),
                end: doc.positionAt(sym.nameSpan.end),
            },
        });
    }
    return symbols;
});

/** Map a 1y SymbolKind to an LSP DocumentSymbol SymbolKind. */
function docSymbolKindFor(k: SymKind): SymbolKind {
    switch (k) {
        case "function": return SymbolKind.Function;
        case "on": return SymbolKind.Method;
        case "let": case "shared": case "state": return SymbolKind.Variable;
        case "type": return SymbolKind.Struct;
        case "enum": return SymbolKind.Enum;
        case "variant": return SymbolKind.EnumMember;
        case "actor": return SymbolKind.Class;
        case "import": return SymbolKind.Module;
        default: return SymbolKind.Variable;
    }
}

// ---------------------------------------------------------------------------
// Definition — jump to declaration nameSpan
// ---------------------------------------------------------------------------

connection.onDefinition((params): Definition | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return null;

    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return null;
    const id = identAt(analysis.tokens, offset);
    if (!id) return null;

    // 1. `module.member` — cross-file jump. Resolve the prefix before the
    //    dot to a module file, then find `member` in that file.
    const crossFile = tryCrossFileDefinition(doc, analysis, offset, id);
    if (crossFile) return crossFile;

    // 2. Same-file resolution.
    let sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) {
        // Cursor on the declaration name itself.
        const onDecl = declAt(analysis.table, offset);
        if (onDecl && onDecl.name === id.name) sym = onDecl;
    }
    if (sym) {
        return {
            uri: doc.uri,
            range: {
                start: doc.positionAt(sym.nameSpan.start),
                end: doc.positionAt(sym.nameSpan.end),
            },
        };
    }

    // 3. Fallback: search the workspace index for a top-level declaration
    //    with this name (e.g. a global from another file imported via
    //    `import lib.yin`).
    if (workspaceIndex) {
        const found = findSymbolAnywhere(workspaceIndex, id.name);
        if (found) {
            return {
                uri: found.uri,
                range: {
                    start: offsetToPosition(found.uri, found.sym.nameSpan.start),
                    end: offsetToPosition(found.uri, found.sym.nameSpan.end),
                },
            };
        }
    }
    return null;
});

/**
 * Resolve a `module.member` definition across files. Returns a Location in
 * the module's file, or null if the prefix isn't a file-backed module.
 */
function tryCrossFileDefinition(
    doc: TextDocument,
    analysis: DocAnalysis,
    offset: number,
    id: { name: string; span: { start: number; end: number } },
): Definition | null {
    if (!workspaceIndex) return null;
    const src = analysis.src;
    // Is the identifier preceded by `.`?
    const dotEnd = id.span.start;
    if (dotEnd < 2 || src[dotEnd - 1] !== ".") return null;
    // Read the prefix identifier before the dot.
    let pEnd = dotEnd - 1;
    let pStart = pEnd;
    while (pStart > 0 && /[A-Za-z0-9_]/.test(src[pStart - 1])) pStart--;
    const prefixName = src.substring(pStart, pEnd);
    if (!prefixName) return null;

    // Resolve the prefix to a module path (via the import symbol).
    const prefixSym = resolveAt(analysis.table, pStart, prefixName)
        ?? analysis.table.decls.find(d => d.name === prefixName && d.kind === "import") ?? null;
    if (!prefixSym || prefixSym.kind !== "import" || !prefixSym.modulePath) return null;

    const targetUri = resolveModuleUri(workspaceIndex, prefixSym.modulePath);
    if (!targetUri) return null; // stdlib module — no file to jump to.

    const found = findSymbolInFile(workspaceIndex, targetUri, id.name);
    if (!found) return null;

    return {
        uri: targetUri,
        range: {
            start: offsetToPosition(targetUri, found.nameSpan.start),
            end: offsetToPosition(targetUri, found.nameSpan.end),
        },
    };
}

/** Convert an offset in a file (by URI) to an LSP Position. */
function offsetToPosition(uri: string, offset: number): Position {
    const fa = workspaceIndex?.files.get(uri);
    if (fa) {
        const line = countLines(fa.src, offset);
        const lineStart = nthLineStart(fa.src, line);
        return { line, character: offset - lineStart };
    }
    // Fallback: treat as offset 0 line if unknown.
    return { line: 0, character: 0 };
}

/** Count the number of `\n` before `offset` in `src`. */
function countLines(src: string, offset: number): number {
    let n = 0;
    for (let i = 0; i < offset && i < src.length; i++) {
        if (src[i] === "\n") n++;
    }
    return n;
}

/** Return the offset of the start of line `line` in `src`. */
function nthLineStart(src: string, line: number): number {
    if (line <= 0) return 0;
    let l = 0;
    for (let i = 0; i < src.length; i++) {
        if (src[i] === "\n") {
            l++;
            if (l === line) return i + 1;
        }
    }
    return src.length;
}

// ---------------------------------------------------------------------------
// Signature help — function call parameter info
// ---------------------------------------------------------------------------

connection.onSignatureHelp((params): SignatureHelp | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return null;

    const { src, table } = analysis;
    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return null;

    // Walk backwards from the cursor to find the enclosing `(` and the
    // callee identifier before it. Track paren depth so nested calls work.
    let depth = 1;
    let lparenOffset = -1;
    let calleeEnd = -1;
    for (let i = offset - 1; i >= 0; i--) {
        const c = src[i];
        if (c === ")") depth++;
        else if (c === "(") {
            depth--;
            if (depth === 0) { lparenOffset = i; break; }
        }
    }
    if (lparenOffset < 0) return null;

    // Find the callee identifier (or `module.func`) ending right before `(`.
    let end = lparenOffset;
    while (end > 0 && /\s/.test(src[end - 1])) end--;
    let start = end;
    // Allow `a.b.c` as the callee.
    while (start > 0 && /[A-Za-z0-9_.]/.test(src[start - 1])) start--;
    // Trim leading dots.
    while (start < end && src[start] === ".") start++;
    const calleeName = src.substring(start, end).split(".").pop() || "";
    if (!calleeName) return null;

    // Count commas between `(` and the cursor to find the active parameter.
    let argIndex = 0;
    let d = 0;
    for (let i = lparenOffset + 1; i < offset; i++) {
        const c = src[i];
        if (c === "(" || c === "[" || c === "{") d++;
        else if (c === ")" || c === "]" || c === "}") d--;
        else if (c === "," && d === 0) argIndex++;
    }

    // Try user-defined symbol first (so a user `count` shadows the builtin).
    const sym = resolveAt(table, lparenOffset, calleeName);
    if (sym && sym.params && sym.params.length > 0) {
        const sigLabel = `${sym.name}${paramsToText(sym.params)}${sym.returnType ? " -> " + annotToText(sym.returnType) : ""}`;
        const paramInfos: ParameterInformation[] = sym.params.map((p) => ({
            label: p.name + (p.typeAnnot ? ": " + annotToText(p.typeAnnot) : ""),
        }));
        return {
            signatures: [{
                label: sigLabel,
                parameters: paramInfos,
                activeParameter: Math.min(argIndex, paramInfos.length - 1),
            }],
        };
    }

    // Fall back to builtin function signatures. The detail string has the
    // form `(T1, T2) -> Ret`; we split it into a param list and a return.
    const builtin = BUILTIN_COMPLETIONS.find((b) => b.label === calleeName);
    if (builtin && builtin.detail) {
        const parsed = parseBuiltinSignature(builtin.label, builtin.detail);
        if (parsed) {
            return {
                signatures: [{
                    label: parsed.label,
                    parameters: parsed.params,
                    activeParameter: Math.min(argIndex, Math.max(parsed.params.length - 1, 0)),
                }],
            };
        }
    }

    // `module.func` builtin (e.g. `json.stringify(`).
    if (src[start - 1] === ".") {
        const modName = resolveModulePrefix(src, start, analysis);
        if (modName) {
            const modItems = MODULE_COMPLETIONS[modName];
            const fnItem = modItems ? modItems.find((it) => it.label === calleeName) : undefined;
            if (fnItem && fnItem.detail) {
                const parsed = parseBuiltinSignature(`${modName}.${calleeName}`, fnItem.detail);
                if (parsed) {
                    return {
                        signatures: [{
                            label: parsed.label,
                            parameters: parsed.params,
                            activeParameter: Math.min(argIndex, Math.max(parsed.params.length - 1, 0)),
                        }],
                    };
                }
            }
        }
    }

    return null;
});

/**
 * Parse a builtin detail string like `(Str, Str) -> Vec` into a signature
 * label + parameter list. Returns null if the detail doesn't match the
 * expected shape.
 */
function parseBuiltinSignature(
    name: string,
    detail: string,
): { label: string; params: ParameterInformation[] } | null {
    const m = /^\(([^)]*)\)\s*(?:->\s*(.*))?$/.exec(detail);
    if (!m) return null;
    const paramText = m[1].trim();
    const ret = m[2] ? m[2].trim() : "";
    const params: ParameterInformation[] = paramText
        ? paramText.split(",").map((p) => ({ label: p.trim() }))
        : [];
    const label = `${name}(${paramText})${ret ? " -> " + ret : ""}`;
    return { label, params };
}

// ---------------------------------------------------------------------------
// References — all occurrences of the resolved symbol's name in scope-visible positions
// ---------------------------------------------------------------------------

connection.onReferences((params): Location[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return [];
    const id = identAt(analysis.tokens, offset);
    if (!id) return [];
    const sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) return [];

    // Find all identifier tokens with the same name that resolve to the same
    // declaration (so shadowed names with the same spelling are excluded).
    const locations: Location[] = [];
    for (const t of analysis.tokens) {
        if (t.type !== TokenType.Ident && t.type !== TokenType.Keyword) continue;
        if (t.text !== sym.name) continue;
        const resolved = resolveAt(analysis.table, t.start, t.text);
        if (!resolved) continue;
        if (resolved.declSpan.start !== sym.declSpan.start) continue;
        locations.push({
            uri: doc.uri,
            range: {
                start: doc.positionAt(t.start),
                end: doc.positionAt(t.end),
            },
        });
    }
    // Include the declaration name itself.
    locations.push({
        uri: doc.uri,
        range: {
            start: doc.positionAt(sym.nameSpan.start),
            end: doc.positionAt(sym.nameSpan.end),
        },
    });
    return locations;
});

// ---------------------------------------------------------------------------
// Document highlight — highlight references under the cursor
// ---------------------------------------------------------------------------

connection.onDocumentHighlight((params): DocumentHighlight[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return [];
    const id = identAt(analysis.tokens, offset);
    if (!id) return [];
    const sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) return [];

    const highlights: DocumentHighlight[] = [];
    for (const t of analysis.tokens) {
        if (t.type !== TokenType.Ident && t.type !== TokenType.Keyword) continue;
        if (t.text !== sym.name) continue;
        const resolved = resolveAt(analysis.table, t.start, t.text);
        if (!resolved) continue;
        if (resolved.declSpan.start !== sym.declSpan.start) continue;
        const isDecl = t.start === sym.nameSpan.start;
        highlights.push({
            range: {
                start: doc.positionAt(t.start),
                end: doc.positionAt(t.end),
            },
            kind: isDecl ? DocumentHighlightKind.Write : DocumentHighlightKind.Read,
        });
    }
    return highlights;
});

// ---------------------------------------------------------------------------
// Rename — rename all references to the resolved symbol
// ---------------------------------------------------------------------------

connection.onPrepareRename((params): Range | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return null;

    const offset = doc.offsetAt(params.position);
    const id = identAt(analysis.tokens, offset);
    if (!id) return null;
    const sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) return null;

    return {
        start: doc.positionAt(id.span.start),
        end: doc.positionAt(id.span.end),
    };
});

connection.onRenameRequest((params): WorkspaceEdit | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return null;

    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return null;
    const id = identAt(analysis.tokens, offset);
    if (!id) return null;
    const sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) return null;
    const newName = params.newName;

    const edits: TextEdit[] = [];
    for (const t of analysis.tokens) {
        if (t.type !== TokenType.Ident && t.type !== TokenType.Keyword) continue;
        if (t.text !== sym.name) continue;
        const resolved = resolveAt(analysis.table, t.start, t.text);
        if (!resolved) continue;
        if (resolved.declSpan.start !== sym.declSpan.start) continue;
        edits.push({
            range: {
                start: doc.positionAt(t.start),
                end: doc.positionAt(t.end),
            },
            newText: newName,
        });
    }
    return {
        changes: { [doc.uri]: edits },
    };
});

// ---------------------------------------------------------------------------
// Semantic tokens — full document coloring
// ---------------------------------------------------------------------------

connection.languages.semanticTokens.on((params): SemanticTokens => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return { data: [] };
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return { data: [] };
    return { data: classifySemanticTokens(analysis.tokens, analysis.table) };
});

// ---------------------------------------------------------------------------
// Inlay hints — inline type hints
// ---------------------------------------------------------------------------

connection.languages.inlayHint.on((params): LspInlayHint[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const hints = buildInlayHints(analysis.table, analysis.typeMap);
    return hints.map((h) => ({
        position: doc.positionAt(h.position),
        label: h.label,
        kind: h.kind,
        paddingLeft: h.paddingLeft,
        paddingRight: h.paddingRight,
    }));
});

// ---------------------------------------------------------------------------
// Workspace symbol search (Ctrl+T)
// ---------------------------------------------------------------------------

connection.onWorkspaceSymbol((params): WorkspaceSymbol[] => {
    if (!workspaceIndex) {
        // Lazily build if the client didn't send folders at init.
        return [];
    }
    const query = (params.query || "").trim();
    const results = searchWorkspaceSymbols(workspaceIndex, query);
    return results.map((r) => ({
        name: r.name,
        kind: workspaceSymbolKindFor(r.kind),
        location: {
            uri: r.uri,
            range: {
                start: offsetToPosition(r.uri, r.nameSpan.start),
                end: offsetToPosition(r.uri, r.nameSpan.end),
            },
        },
        containerName: r.containerName,
    }));
});

/** Map a 1y SymbolKind to an LSP SymbolKind for workspace symbol results. */
function workspaceSymbolKindFor(k: WsSymKind): SymbolKind {
    switch (k) {
        case "function": return SymbolKind.Function;
        case "on": return SymbolKind.Method;
        case "let": case "shared": case "state": return SymbolKind.Variable;
        case "type": return SymbolKind.Struct;
        case "enum": return SymbolKind.Enum;
        case "variant": return SymbolKind.EnumMember;
        case "actor": return SymbolKind.Class;
        case "import": return SymbolKind.Module;
        default: return SymbolKind.Variable;
    }
}

// ---------------------------------------------------------------------------
// Folding ranges — fold function bodies, blocks, imports, comments
// ---------------------------------------------------------------------------

connection.onFoldingRanges((params): FoldingRange[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];
    const src = analysis.src;
    // Precompute offset→line via TextDocument for accuracy.
    const lineOf = (offset: number) => doc.positionAt(offset).line;
    const astRanges = buildFoldingRanges(analysis.program, lineOf);
    const extraRanges = buildCommentAndImportFolds(src, lineOf);
    const out: FoldingRange[] = [];
    for (const r of [...astRanges, ...extraRanges]) {
        out.push({
            startLine: r.startLine,
            endLine: r.endLine,
            kind: r.kind,
        });
    }
    return out;
});

// ---------------------------------------------------------------------------
// Selection range — expand selection outward through AST
// ---------------------------------------------------------------------------

connection.onSelectionRanges((params): SelectionRange[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const results: SelectionRange[] = [];
    for (const pos of params.positions) {
        const offset = doc.offsetAt(pos);
        const chain = buildSelectionRange(analysis.program, offset);
        if (!chain) continue;
        // Convert the linked list of Span-based nodes into LSP SelectionRange.
        const convert = (node: { range: { start: number; end: number }; parent?: any } | undefined): SelectionRange | null => {
            if (!node) return null;
            const inner = convert(node.parent);
            return {
                range: {
                    start: doc.positionAt(node.range.start),
                    end: doc.positionAt(node.range.end),
                },
                parent: inner ?? undefined,
            };
        };
        const sr = convert(chain as any);
        if (sr) results.push(sr);
    }
    return results;
});

// ---------------------------------------------------------------------------
// Type definition — jump to the type declaration of a symbol's annotation
// ---------------------------------------------------------------------------

connection.onTypeDefinition((params): Definition | null => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return null;
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return null;

    const offset = doc.offsetAt(params.position);
    if (isInStringOrComment(analysis.tokens, offset)) return null;
    const id = identAt(analysis.tokens, offset);
    if (!id) return null;

    // Resolve the identifier. If it's itself a type/enum/actor, jump to it.
    let sym = resolveAt(analysis.table, offset, id.name);
    if (!sym) {
        const onDecl = declAt(analysis.table, offset);
        if (onDecl && onDecl.name === id.name) sym = onDecl;
    }
    if (!sym) return null;

    // If the symbol IS a type definition, jump to it.
    if (sym.kind === "type" || sym.kind === "enum" || sym.kind === "actor" || sym.kind === "variant") {
        return {
            uri: doc.uri,
            range: {
                start: doc.positionAt(sym.nameSpan.start),
                end: doc.positionAt(sym.nameSpan.end),
            },
        };
    }

    // Otherwise, look at the symbol's type annotation. If it names a user
    // type (e.g. `let p: Point`), jump to that type's declaration.
    if (sym.typeAnnot) {
        const typeName = typeNameOf(sym.typeAnnot);
        if (typeName) {
            const typeSym = analysis.table.decls.find(
                d => d.name === typeName && (d.kind === "type" || d.kind === "enum" || d.kind === "actor"));
            if (typeSym) {
                return {
                    uri: doc.uri,
                    range: {
                        start: doc.positionAt(typeSym.nameSpan.start),
                        end: doc.positionAt(typeSym.nameSpan.end),
                    },
                };
            }
            // Cross-file type lookup.
            if (workspaceIndex) {
                const found = findSymbolAnywhere(workspaceIndex, typeName);
                if (found && (found.sym.kind === "type" || found.sym.kind === "enum" || found.sym.kind === "actor")) {
                    return {
                        uri: found.uri,
                        range: {
                            start: offsetToPosition(found.uri, found.sym.nameSpan.start),
                            end: offsetToPosition(found.uri, found.sym.nameSpan.end),
                        },
                    };
                }
            }
        }
    }
    return null;
});

/** Extract the user-type name from a TypeAnnot, or null if not a Name. */
function typeNameOf(t: { kind: string; name?: string }): string | null {
    if (t.kind === "Name") return t.name || null;
    if (t.kind === "Generic") return t.name || null; // Vec<Int> → "Vec"
    return null;
}

// ---------------------------------------------------------------------------
// Code lens — show reference count above each top-level function/handler
// ---------------------------------------------------------------------------

connection.onCodeLens((params): CodeLens[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const analysis = docAnalysis.get(doc.uri);
    if (!analysis) return [];

    const lenses: CodeLens[] = [];
    for (const sym of analysis.table.decls) {
        if (sym.scopeStart !== 0) continue; // top-level only
        if (sym.kind !== "function" && sym.kind !== "on") continue;

        // Count references in this file (same logic as onReferences).
        let count = 0;
        for (const t of analysis.tokens) {
            if (t.type !== TokenType.Ident && t.type !== TokenType.Keyword) continue;
            if (t.text !== sym.name) continue;
            const resolved = resolveAt(analysis.table, t.start, t.text);
            if (resolved && resolved.declSpan.start === sym.declSpan.start) count++;
        }

        lenses.push({
            range: {
                start: doc.positionAt(sym.nameSpan.start),
                end: doc.positionAt(sym.nameSpan.end),
            },
            // Store the symbol name + decl offset so the resolver can build
            // a "Find References" command target.
            data: JSON.stringify({ name: sym.name, declStart: sym.declSpan.start, refs: count }),
        });
    }
    return lenses;
});

// ---------------------------------------------------------------------------
// Document link — make URLs in comments clickable
// ---------------------------------------------------------------------------

connection.onDocumentLinks((params): DocumentLink[] => {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return [];
    const src = doc.getText();
    const links: DocumentLink[] = [];

    // Scan line by line for `http://` or `https://` URLs in comments.
    const urlRe = /(https?:\/\/[^\s"'`)\]]+)/g;
    const lines = src.split(/\r?\n/);
    for (let lineIdx = 0; lineIdx < lines.length; lineIdx++) {
        const line = lines[lineIdx];
        let m: RegExpExecArray | null;
        while ((m = urlRe.exec(line)) !== null) {
            const url = m[1];
            const charStart = m.index;
            const charEnd = charStart + url.length;
            links.push({
                range: {
                    start: { line: lineIdx, character: charStart },
                    end: { line: lineIdx, character: charEnd },
                },
                target: url,
            });
        }
    }
    return links;
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
