// Verify folding ranges + selection ranges + code lens + document links.
import { readFileSync } from "fs";
import { tokenize } from "../src/lexer";
import { parse } from "../src/parser";
import { attachDocStrings } from "../src/docstrings";
import { buildSymbolTable } from "../src/symbols";
import { buildTypeMap } from "../src/types";
import { buildFoldingRanges, buildCommentAndImportFolds } from "../src/foldingRange";
import { buildSelectionRange } from "../src/selectionRange";

const file = process.argv[2] || "../examples/lib/yin.1y";
const src = readFileSync(file, "utf-8");
const toks = tokenize(src);
const r = parse(src);
attachDocStrings(src, r.program);
const table = buildSymbolTable(src, r.program);
const tm = buildTypeMap(r.program, table);

const lineOf = (off: number) => src.slice(0, off).split("\n").length - 1;

const folds = buildFoldingRanges(r.program, lineOf);
const extra = buildCommentAndImportFolds(src, lineOf);
console.log(`folding ranges: ${folds.length} AST + ${extra.length} comment/import`);
for (const f of [...folds, ...extra].slice(0, 8)) {
    console.log(`  L${f.startLine}-L${f.endLine} [${f.kind || "region"}]`);
}

console.log("\nselection range at offset 1500:");
const sr = buildSelectionRange(r.program, 1500);
let depth = 0;
for (let n: any = sr; n; n = n.parent) {
    console.log(`  ${"  ".repeat(depth)}[${n.range.start}-${n.range.end}] len=${n.range.end - n.range.start}`);
    depth++;
    if (depth > 6) break;
}

console.log("\ntop-level functions (for code lens):");
let fnCount = 0;
for (const s of table.decls) {
    if (s.scopeStart === 0 && (s.kind === "function" || s.kind === "on")) {
        fnCount++;
        if (fnCount <= 5) console.log(`  ${s.name} [${s.kind}] @${s.nameSpan.start}`);
    }
}
console.log(`  ... total: ${fnCount}`);
