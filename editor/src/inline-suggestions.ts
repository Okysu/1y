// Inline suggestions (ghost text) for the 1y language.
//
// Uses VSCode's `InlineCompletionItemProvider` API (stable since 1.67) to
// show context-aware completions as greyed-out text after the cursor. Press
// Tab to accept.
//
// Strategy: lightweight heuristic on the current line + a small look-back.
// We DON'T attempt full semantic completion (that's the LSP completion
// provider's job). Instead we cover the "finish the construct" cases that
// users typically want to auto-complete:
//
//   - `let x`             → ` = ${cursor};`
//   - `fn name(`          → `)`
//   - `fn name() ->`      → ` Type {`
//   - `match x {` + EOL   → `\n\t_ => Nil,\n` (wildcard arm)
//   - `if cond {` + EOL   → `\n\t\n}` (empty body)
//   - `spawn(`            → `)`
//   - `transact {` + EOL  → `\n\t\n}`
//   - `actor Name {`      → ` state: ..., on ...`
//   - `println(`          → `)`
//   - `import ` + Ident   → `;`
//
// Each suggestion is computed from the line text up to the cursor; we never
// look at later text. If the text to the right already completes the
// construct, VSCode automatically hides the ghost text.

import {
    CancellationToken,
    Disposable,
    ExtensionContext,
    InlineCompletionItem,
    InlineCompletionItemProvider,
    InlineCompletionList,
    InlineCompletionContext,
    Position,
    Range,
    TextDocument,
    languages,
} from "vscode";

export class InlineSuggestionProvider implements InlineCompletionItemProvider, Disposable {
    private readonly disposables: Disposable[] = [];

    constructor(private readonly context: ExtensionContext) {}

    activate(): void {
        this.disposables.push(
            languages.registerInlineCompletionItemProvider(
                { pattern: "**/*.1y" },
                this,
            ),
        );
    }

    dispose(): void {
        for (const d of this.disposables) {
            try { d.dispose(); } catch { /* ignore */ }
        }
        this.disposables.length = 0;
    }

    async provideInlineCompletionItems(
        document: TextDocument,
        position: Position,
        _context: InlineCompletionContext,
        _token: CancellationToken,
    ): Promise<InlineCompletionItem[] | InlineCompletionList | undefined> {
        const line = document.lineAt(position.line).text;
        const prefix = line.substring(0, position.character);

        // Don't fire inside strings or comments.
        if (isInsideStringOrComment(prefix)) return undefined;

        const suggestion = suggestForPrefix(prefix);
        if (!suggestion) return undefined;

        const item = new InlineCompletionItem(
            suggestion,
            new Range(position, position),
        );
        return [item];
    }
}

/**
 * Decide what ghost text (if any) to suggest for the current line prefix.
 * Returns the text to insert, or undefined for "no suggestion".
 */
function suggestForPrefix(prefix: string): string | undefined {
    // Strip trailing whitespace for matching, but keep the original for insertion.
    const trimmed = prefix.replace(/\s+$/, "");
    if (!trimmed) return undefined;

    // --- `let x` (no `=` yet) → suggest ` = ` ----------------------------
    // Match `let <ident>` not followed by anything else meaningful.
    let m = /^\s*let\s+([A-Za-z_]\w*)\s*$/.exec(trimmed);
    if (m) {
        // Don't suggest if there's already an `=` (we matched `let x`, so no).
        return " = ";
    }

    // --- `let x = ` (incomplete RHS, suggest snippet) ---------------------
    m = /^\s*let\s+([A-Za-z_]\w*)\s*=\s*$/.exec(trimmed);
    if (m) {
        // Suggest a placeholder expression + semicolon.
        return "expr;";
    }

    // --- `fn name(` → suggest `)` if not already present -----------------
    m = /^\s*fn\s+([A-Za-z_]\w*)\s*\(([^)]*)$/.exec(trimmed);
    if (m) {
        // Only suggest if the parenthesis is not yet closed on this line.
        return ")";
    }

    // --- `fn name() -> ` → suggest type + body ---------------------------
    m = /^\s*fn\s+([A-Za-z_]\w*)\s*\([^)]*\)\s*->\s*$/.exec(trimmed);
    if (m) {
        return "Type {\n\t\n}";
    }

    // --- `fn name()` (no body) → suggest ` { ... }` ----------------------
    m = /^\s*fn\s+([A-Za-z_]\w*)\s*\([^)]*\)\s*$/.exec(trimmed);
    if (m) {
        return " {";
    }

    // --- `match expr {` (line ends with `{`) → wildcard arm --------------
    m = /^\s*match\s+.+\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t_ => Nil,\n";
    }

    // --- `if cond {` → suggest body + close brace ------------------------
    m = /^\s*if\s+.+\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n}";
    }

    // --- `while cond {` → suggest body -----------------------------------
    m = /^\s*while\s+.+\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n}";
    }

    // --- `for x in iter {` → suggest body --------------------------------
    m = /^\s*for\s+.+\s+in\s+.+\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n}";
    }

    // --- `loop {` → suggest body -----------------------------------------
    m = /^\s*loop\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n}";
    }

    // --- `transact {` → suggest body -------------------------------------
    m = /^\s*transact\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n}";
    }

    // --- `try {` → suggest body + rescue ---------------------------------
    m = /^\s*try\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\t\n} rescue as e {\n\t\n}";
    }

    // --- `actor Name {` → suggest boilerplate ----------------------------
    m = /^\s*actor\s+([A-Z]\w*)\s*\{\s*$/.exec(trimmed);
    if (m) {
        return `\n\tstate: Nil,\n\ton Init(args) {\n\t\treply Nil;\n\t}\n`;
    }

    // --- `enum Name {` → suggest first variant ---------------------------
    m = /^\s*enum\s+([A-Z]\w*)\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\tVariant,\n";
    }

    // --- `type Name = {` → suggest field --------------------------------
    m = /^\s*type\s+([A-Z]\w*)\s*=\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\tfield: Type,\n";
    }

    // --- `spawn(` → suggest `)` ------------------------------------------
    m = /^\s*spawn\s*\(([^)]*)$/.exec(trimmed);
    if (m) {
        return ")";
    }

    // --- `receive {` → suggest first arm ---------------------------------
    m = /^\s*receive\s*\{\s*$/.exec(trimmed);
    if (m) {
        return "\n\tMsg => reply Nil,\n";
    }

    // --- `import <ident>` → suggest `;` ----------------------------------
    m = /^\s*(lazy\s+)?import\s+([A-Za-z_]\w*)\s*$/.exec(trimmed);
    if (m) {
        return ";";
    }

    // --- bare builtin calls with open paren: `println(` → `)` ------------
    m = /^\s*(println|print|count|first|rest|push|cons|assoc|dissoc|get|map|filter|fold|reduce|find|each|len|split|join|replace|trim|contains|substring|str|int|pow|abs|min|max|sqrt)\s*\(([^)]*)$/.exec(trimmed);
    if (m) {
        return ")";
    }

    // --- `module.` → suggest likely first function (heuristic, low-confidence) ---
    // We skip this case: the LSP completion provider already handles it
    // via trigger characters, and a default first-function suggestion would
    // be too opinionated.

    return undefined;
}

/** True if the line prefix so far is inside a string literal or comment. */
function isInsideStringOrComment(prefix: string): boolean {
    let i = 0;
    let inString = false;
    let inLineComment = false;
    let inBlockCommentDepth = 0;
    while (i < prefix.length) {
        const c = prefix[i];
        const next = prefix[i + 1] || "";

        if (inLineComment) {
            // Line comments end at EOL — but we're scanning a single line
            // prefix, so if we got here, we're still in the comment.
            return true;
        }
        if (inBlockCommentDepth > 0) {
            if (c === "*" && next === "/") { inBlockCommentDepth--; i += 2; continue; }
            if (c === "/" && next === "*") { inBlockCommentDepth++; i += 2; continue; }
            i++;
            continue;
        }
        if (inString) {
            if (c === "\\") { i += 2; continue; }
            if (c === '"') { inString = false; i++; continue; }
            i++;
            continue;
        }
        // not in string/comment
        if (c === "/" && next === "/") return true;
        if (c === "/" && next === "*") { inBlockCommentDepth = 1; i += 2; continue; }
        if (c === '"') { inString = true; i++; continue; }
        i++;
    }
    return inString || inBlockCommentDepth > 0;
}
