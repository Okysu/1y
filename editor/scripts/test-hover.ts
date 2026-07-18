// Verify hover content for user functions (no docstring), variables (with
// inferred types), and import aliases (`import json as J`). Run with:
//   npx ts-node scripts/test-hover.ts <file>
import { readFileSync } from "fs";
import { parse } from "../src/parser";
import { attachDocStrings } from "../src/docstrings";
import { buildSymbolTable, resolveAt, declAt } from "../src/symbols";
import { buildTypeMap, symbolType, typeToText } from "../src/types";

const file = process.argv[2] || "../examples/lib/yin.1y";
const src = readFileSync(file, "utf-8");
const r = parse(src);
attachDocStrings(src, r.program);
const table = buildSymbolTable(src, r.program);
const tm = buildTypeMap(r.program, table);

function probe(name: string, atOffset: number): void {
    // Mirror server.ts hover logic: resolveAt first, declAt fallback so
    // hovering on a declaration's own name also resolves.
    let sym = resolveAt(table, atOffset, name);
    if (!sym) {
        const onDecl = declAt(table, atOffset);
        if (onDecl && onDecl.name === name) sym = onDecl;
    }
    if (!sym) {
        console.log(`${name} @${atOffset}: <unresolved>`);
        return;
    }
    const t = symbolType(sym, tm);
    const typeText = t.kind !== "Unknown" ? typeToText(t) : "<no type>";
    console.log(`${name} @${atOffset}: kind=${sym.kind}, type=${typeText}, doc=${JSON.stringify(sym.doc.slice(0, 40))}${sym.modulePath ? ", module=" + sym.modulePath : ""}`);
}

console.log(`=== ${file} ===`);
console.log(`decls: ${table.decls.length}`);

// Probe import aliases.
console.log("\n-- import aliases --");
for (const d of table.decls) {
    if (d.kind === "import") {
        console.log(`  ${d.name}: modulePath=${d.modulePath || "<none>"}`);
    }
}

// Probe a few user functions / lets at their use sites.
console.log("\n-- user symbols at use-sites --");
// Find the first occurrence of `J.` to probe the alias in use.
const jUse = src.indexOf("J.");
if (jUse >= 0) probe("J", jUse);

// Probe `group` (a documented function).
const groupUse = src.indexOf("group(");
if (groupUse >= 0) probe("group", groupUse);

// Probe `register` (likely no docstring).
const regUse = src.indexOf("register(");
if (regUse >= 0) probe("register", regUse);

// Probe `match_route` (returns a Map).
const mrUse = src.indexOf("match_route(");
if (mrUse >= 0) probe("match_route", mrUse);

// Probe `split_path` (top-level let, returns Vec).
const spUse = src.indexOf("split_path(");
if (spUse >= 0) probe("split_path", spUse);
