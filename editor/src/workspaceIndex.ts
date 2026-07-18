// Workspace-wide index for cross-file navigation.
//
// Two responsibilities:
//   1. Module path → file URI resolution: `import lib.yin` → `lib/yin.1y`.
//      Stdlib modules (json, io, process, ...) have no file and resolve to
//      null — they're served from MODULE_COMPLETIONS instead.
//   2. Workspace symbol search (Ctrl+T): a flat list of top-level
//      declarations across all `.1y` files in the workspace, with locations.
//
// The server runs in Node, so we read files directly via `fs`. The index is
// built lazily on first use and rebuilt when files change (the server calls
// `refreshFile` / `removeFile` on didOpen/didChange/didClose for tracked
// documents, and rescans the whole tree when a workspace symbol query comes
// in and the index is stale).

import { readFileSync, existsSync, readdirSync, statSync } from "fs";
import { join, extname, sep } from "path";
import { pathToFileURL } from "url";
import { parse } from "./parser";
import { attachDocStrings } from "./docstrings";
import { buildSymbolTable } from "./symbols";
import { buildTypeMap } from "./types";
import { Program, Span } from "./ast";
import { SymbolTable, SymbolInfo } from "./symbols";
import { TypeMap } from "./types";
import { SymbolKind as SymKind } from "./symbols";

// Re-export for server convenience.
export { SymKind };
export type { SymbolInfo, SymbolTable, TypeMap };

/** A top-level symbol entry for workspace symbol search. */
export interface WorkspaceSymbolEntry {
    name: string;
    kind: SymKind;
    uri: string;
    /** Location of the declaration name (jump target). */
    nameSpan: Span;
    doc: string;
    /** Container name (e.g. actor name for an `on` handler), if any. */
    containerName?: string;
}

/** Cached analysis for a single file (parsed + symbols + types). */
export interface FileAnalysis {
    uri: string;
    src: string;
    program: Program;
    table: SymbolTable;
    typeMap: TypeMap;
}

/** Per-workspace index. */
export interface WorkspaceIndex {
    /** Workspace root folders (absolute paths). */
    roots: string[];
    /** file URI → FileAnalysis (cached). */
    files: Map<string, FileAnalysis>;
    /** module path → file URI (e.g. "lib.yin" → "file:///.../lib/yin.1y"). */
    moduleToUri: Map<string, string>;
    /** Flat top-level symbol list across all files. */
    symbols: WorkspaceSymbolEntry[];
    /** Monotonic version — bumped on every change so callers can cache. */
    version: number;
}

const FILE_EXT = ".1y";

/** Build a fresh index by scanning the workspace roots. */
export function buildWorkspaceIndex(roots: string[]): WorkspaceIndex {
    const index: WorkspaceIndex = {
        roots,
        files: new Map(),
        moduleToUri: new Map(),
        symbols: [],
        version: 0,
    };
    for (const root of roots) indexFileTree(index, root);
    rebuildDerived(index);
    return index;
}

/** Recursively scan a directory tree and add every `.1y` file. */
function indexFileTree(index: WorkspaceIndex, dir: string): void {
    let entries: string[];
    try {
        entries = readdirSync(dir);
    } catch {
        return; // permission error or missing dir — skip silently.
    }
    for (const name of entries) {
        // Skip common noise directories.
        if (name === "node_modules" || name === ".git" || name.startsWith(".")) continue;
        const full = join(dir, name);
        let st: { isDirectory: () => boolean; isFile: () => boolean };
        try {
            st = statSync(full);
        } catch {
            continue;
        }
        if (st.isDirectory()) {
            indexFileTree(index, full);
        } else if (st.isFile() && extname(name) === FILE_EXT) {
            addFileToIndex(index, full);
        }
    }
}

/** Read and parse a file, adding it to the index. Path is absolute. */
function addFileToIndex(index: WorkspaceIndex, absPath: string): void {
    let src: string;
    try {
        src = readFileSync(absPath, "utf-8");
    } catch {
        return;
    }
    const uri = pathToFileURL(absPath).href;
    const r = parse(src);
    attachDocStrings(src, r.program);
    const table = buildSymbolTable(src, r.program);
    const typeMap = buildTypeMap(r.program, table);
    index.files.set(uri, { uri, src, program: r.program, table, typeMap });
}

/**
 * Refresh a single file's analysis from its current text. Called by the
 * server on didOpen/didChange for tracked documents so the index stays in
 * sync without re-reading from disk.
 */
export function refreshFile(
    index: WorkspaceIndex,
    uri: string,
    src: string,
): void {
    const r = parse(src);
    attachDocStrings(src, r.program);
    const table = buildSymbolTable(src, r.program);
    const typeMap = buildTypeMap(r.program, table);
    index.files.set(uri, { uri, src, program: r.program, table, typeMap });
    rebuildDerived(index);
}

/** Remove a file from the index (on didClose). */
export function removeFile(index: WorkspaceIndex, uri: string): void {
    index.files.delete(uri);
    rebuildDerived(index);
}

/**
 * Rebuild the module→uri map and the flat symbol list from the cached file
 * analyses. Called after any file add/remove/refresh.
 */
function rebuildDerived(index: WorkspaceIndex): void {
    index.moduleToUri.clear();
    index.symbols = [];
    for (const [uri, fa] of index.files) {
        // Module path = file path relative to a workspace root, without
        // extension, with OS separators → dots. e.g. `lib/yin.1y` → `lib.yin`.
        const modPath = uriToModulePath(uri, index.roots);
        if (modPath) index.moduleToUri.set(modPath, uri);

        for (const sym of fa.table.decls) {
            if (sym.scopeStart !== 0) continue; // top-level only
            index.symbols.push({
                name: sym.name,
                kind: sym.kind,
                uri,
                nameSpan: sym.nameSpan,
                doc: sym.doc,
            });
        }
    }
    index.version++;
}

/** Convert a file URI to a dotted module path relative to a workspace root. */
function uriToModulePath(uri: string, roots: string[]): string | null {
    let path: string;
    try {
        path = fileURLToPathSafe(uri);
    } catch {
        return null;
    }
    for (const root of roots) {
        if (path.startsWith(root)) {
            let rel = path.slice(root.length);
            // Strip leading separator.
            rel = rel.replace(/^[\\/]+/, "");
            // Drop extension.
            rel = rel.replace(new RegExp(FILE_EXT.replace(/\./g, "\\.") + "$"), "");
            // Separators → dots.
            return rel.replace(/[\\/]+/g, ".");
        }
    }
    return null;
}

/** Best-effort fileURLToPath without importing the `url` helper everywhere. */
function fileURLToPathSafe(uri: string): string {
    // `file:///C:/foo` → `C:/foo` on Windows; `file:///foo` → `/foo` elsewhere.
    const m = /^file:\/\/\/(.*)$/.exec(uri);
    if (!m) return uri;
    let p = decodeURIComponent(m[1]);
    // Windows drive letter: `C:/...` stays; otherwise re-add leading slash.
    if (!/^[A-Za-z]:[\\/]/.test(p)) p = "/" + p;
    return p;
}

// ---------------------------------------------------------------------------
// Module path → file URI resolution
// ---------------------------------------------------------------------------

/**
 * Resolve a 1y module path to a file URI. Stdlib modules (json, io, ...)
 * have no file and return null. `lib.yin` → `<root>/lib/yin.1y` → file URI.
 */
export function resolveModuleUri(
    index: WorkspaceIndex,
    modulePath: string,
): string | null {
    // Fast path: already indexed.
    const cached = index.moduleToUri.get(modulePath);
    if (cached) return cached;

    // Try each workspace root.
    const relFile = modulePath.replace(/\./g, sep) + FILE_EXT;
    for (const root of rootsAsPaths(index)) {
        const candidate = join(root, relFile);
        if (existsSync(candidate)) {
            // Lazily index the file so subsequent lookups are fast.
            addFileToIndex(index, candidate);
            const uri = pathToFileURL(candidate).href;
            index.moduleToUri.set(modulePath, uri);
            rebuildDerived(index);
            return uri;
        }
    }
    return null;
}

/** Return workspace roots as OS paths (decoded from any URI form). */
function rootsAsPaths(index: WorkspaceIndex): string[] {
    return index.roots;
}

// ---------------------------------------------------------------------------
// Cross-file symbol lookup
// ---------------------------------------------------------------------------

/**
 * Find a top-level symbol by name in a specific file (by URI). Used by
 * cross-file go-to-definition: resolve the module, then find the member.
 */
export function findSymbolInFile(
    index: WorkspaceIndex,
    uri: string,
    name: string,
): SymbolInfo | null {
    const fa = index.files.get(uri);
    if (!fa) return null;
    for (const sym of fa.table.decls) {
        if (sym.scopeStart !== 0) continue;
        if (sym.name === name) return sym;
    }
    return null;
}

/**
 * Find a top-level symbol by name across all indexed files. Used as a
 * fallback for go-to-definition when the name isn't visible in the current
 * file's scope (e.g. an unresolved global from another module).
 */
export function findSymbolAnywhere(
    index: WorkspaceIndex,
    name: string,
): { sym: SymbolInfo; uri: string } | null {
    for (const [uri, fa] of index.files) {
        for (const sym of fa.table.decls) {
            if (sym.scopeStart !== 0) continue;
            if (sym.name === name) return { sym, uri };
        }
    }
    return null;
}

// ---------------------------------------------------------------------------
// Workspace symbol search (Ctrl+T)
// ---------------------------------------------------------------------------

/**
 * Search the workspace index for symbols matching `query`. Matching is
 * case-insensitive substring on the symbol name. Results are sorted by
 * relevance (exact match first, then prefix, then substring).
 */
export function searchWorkspaceSymbols(
    index: WorkspaceIndex,
    query: string,
): WorkspaceSymbolEntry[] {
    const q = query.toLowerCase();
    const results: WorkspaceSymbolEntry[] = [];
    for (const sym of index.symbols) {
        const name = sym.name.toLowerCase();
        if (!q || name.includes(q)) {
            results.push(sym);
        }
    }
    // Sort: exact > prefix > substring; shorter name first as a tiebreak.
    results.sort((a, b) => {
        const an = a.name.toLowerCase();
        const bn = b.name.toLowerCase();
        const aExact = an === q ? 0 : 1;
        const bExact = bn === q ? 0 : 1;
        if (aExact !== bExact) return aExact - bExact;
        const aPrefix = an.startsWith(q) ? 0 : 1;
        const bPrefix = bn.startsWith(q) ? 0 : 1;
        if (aPrefix !== bPrefix) return aPrefix - bPrefix;
        return a.name.length - b.name.length;
    });
    return results;
}
