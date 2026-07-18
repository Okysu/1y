// Inlay hints for the 1y language server.
//
// Drives `textDocument/inlayHint`: shows inferred types inline at declaration
// sites that lack a written annotation, like Pylance/Rust-analyzer:
//
//   let x = 42        // shows `: Int` after `x`
//   fn add(a, b) ...  // shows `: ?` after `a` and `b`
//   let v = [1, 2]    // shows `: Vec<Int>` after `v`
//
// Only emits a hint when the type is actually inferable (non-Unknown); we
// don't spam `: ?` everywhere. Hints are purely visual and never modify the
// source.
//
// Implementation iterates the symbol table's flat declaration list — it
// already contains every binding (global, nested let, params, lambda params,
// for/match vars) with its nameSpan and typeAnnot, so no separate AST walk
// is needed.

import { SymbolTable } from "./symbols";
import { TypeMap, symbolType, typeToText } from "./types";

export interface InlayHint {
    /** Character offset where the hint label is inserted (after the name). */
    position: number;
    label: string;
    kind: 1; // Type
    paddingLeft?: boolean;
    paddingRight?: boolean;
}

/** Build inlay hints for the whole program from the symbol table. */
export function buildInlayHints(
    table: SymbolTable,
    typeMap: TypeMap,
): InlayHint[] {
    const hints: InlayHint[] = [];
    for (const sym of table.decls) {
        // Skip declarations with an explicit annotation — no hint needed.
        if (sym.typeAnnot) continue;

        // Only annotate bindings/params, not type/enum/actor/import defs.
        switch (sym.kind) {
            case "let":
            case "shared":
            case "state":
            case "param":
            case "lambda_param":
            case "for_var":
            case "match_bind":
                break; // eligible
            default:
                continue;
        }

        const t = symbolType(sym, typeMap);
        if (t.kind === "Unknown") continue;

        hints.push({
            position: sym.nameSpan.end,
            label: `: ${typeToText(t)}`,
            kind: 1,
            paddingLeft: false,
        });
    }
    return hints;
}
