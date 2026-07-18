// Quick test for type inference — run with: npx ts-node scripts/test-types.ts
import { readFileSync } from "fs";
import { parse } from "../src/parser";
import { attachDocStrings } from "../src/docstrings";
import { buildSymbolTable, resolveAt } from "../src/symbols";
import { buildTypeMap, symbolType, typeToText } from "../src/types";

const file = process.argv[2] || "../examples/phase1.1y";
const src = readFileSync(file, "utf-8");
const r = parse(src);
attachDocStrings(src, r.program);
const t = buildSymbolTable(src, r.program);
const tm = buildTypeMap(r.program, t);

function show(name: string, pos: number): void {
    const s = resolveAt(t, pos, name);
    if (!s) { console.log(`${name}: <unresolved>`); return; }
    console.log(`${name}: ${typeToText(symbolType(s, tm))}  [${s.kind}]`);
}

console.log(`=== ${file} ===`);
console.log(`decls: ${t.decls.length}, inferred types: ${tm.inferred.size}`);

// Show inferred types for all top-level declarations
console.log("\n-- top-level types --");
for (const d of t.decls) {
    if (d.scopeStart === 0 && (d.kind === "function" || d.kind === "let" || d.kind === "type" || d.kind === "enum" || d.kind === "variant")) {
        console.log(`  ${d.kind} ${d.name}: ${typeToText(symbolType(d, tm))}`);
    }
}

// Test specific resolutions
console.log("\n-- resolution tests --");
show("factorial", src.indexOf("factorial(n - 1)"));
show("v", src.indexOf("count(v)"));
show("add10", src.indexOf("add10(5)"));
show("fib", src.indexOf("fib(15)"));
show("Point", src.indexOf("Point({ x: 3"));
show("Circle", src.indexOf("Circle(5)"));
show("area", src.indexOf("area(Circle(5))"));
show("result", src.indexOf("println(result)"));
