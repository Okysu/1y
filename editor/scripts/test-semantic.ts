// Verify semantic tokens + inlay hints on a sample file.
import { readFileSync } from "fs";
import { tokenize } from "../src/lexer";
import { parse } from "../src/parser";
import { attachDocStrings } from "../src/docstrings";
import { buildSymbolTable } from "../src/symbols";
import { buildTypeMap } from "../src/types";
import { classifySemanticTokens, SEMANTIC_TOKEN_TYPES } from "../src/semanticTokens";
import { buildInlayHints } from "../src/inlayHints";

const file = process.argv[2] || "../examples/phase1.1y";
const src = readFileSync(file, "utf-8");
const toks = tokenize(src);
const r = parse(src);
attachDocStrings(src, r.program);
const table = buildSymbolTable(src, r.program);
const tm = buildTypeMap(r.program, table);

const data = classifySemanticTokens(toks, table);
console.log(`semantic tokens: ${data.length / 5} classified`);
// Count by type.
const counts: Record<string, number> = {};
for (let i = 0; i < data.length; i += 5) {
    const t = SEMANTIC_TOKEN_TYPES[data[i + 3]];
    counts[t] = (counts[t] || 0) + 1;
}
console.log("by type:", counts);

const hints = buildInlayHints(table, tm);
console.log(`\ninlay hints: ${hints.length}`);
for (const h of hints.slice(0, 20)) {
    const line = src.slice(0, h.position).split("\n").length;
    console.log(`  L${line}: ${h.label}`);
}
