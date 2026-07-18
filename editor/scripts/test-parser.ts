// Quick sanity test for the parser — parses a file and prints a summary.
// Run with: npx ts-node scripts/test-parser.ts <file>
import { readFileSync } from "fs";
import { parse } from "../src/parser";

const file = process.argv[2] || "../examples/stm_bank.1y";
const src = readFileSync(file, "utf-8");
const result = parse(src);

console.log(`parsed ${file}`);
console.log(`  statements: ${result.program.stmts.length}`);
console.log(`  errors: ${result.errors.length}`);
for (const e of result.errors.slice(0, 10)) {
    console.log(`    [${e.span.start}-${e.span.end}] ${e.message}`);
}

function summarize(stmt: any, depth = 0): string {
    const pad = "  ".repeat(depth);
    switch (stmt.kind) {
        case "FuncDef":
            return `${pad}fn ${stmt.name}(${stmt.params.map((p: any) => p.name).join(", ")}) -> ${stmt.returnType ? "T" : "?"}`;
        case "Let":
            return `${pad}let ${stmt.name}`;
        case "SharedDecl":
            return `${pad}shared ${stmt.name}`;
        case "ActorDef":
            return `${pad}actor ${stmt.name} { ${stmt.body.length} members }`;
        case "OnClause":
            return `${pad}on ${stmt.name}(${stmt.params.map((p: any) => p.name).join(", ")})`;
        case "EnumDef":
            return `${pad}enum ${stmt.name} { ${stmt.variants.map((v: any) => v.name).join(", ")} }`;
        case "TypeDef":
            return `${pad}type ${stmt.name} { ${stmt.fields.map((f: any) => f.name).join(", ")} }`;
        case "Import":
            return `${pad}import ${stmt.path}${stmt.alias ? " as " + stmt.alias : ""}`;
        case "Expr":
            return `${pad}expr: ${stmt.expr.kind}`;
        case "Semi":
            return `${pad}semi: ${stmt.expr.kind}`;
        default:
            return `${pad}${stmt.kind}`;
    }
}

console.log("\ntop-level:");
for (const s of result.program.stmts) {
    console.log(summarize(s));
}

// Count nested nodes for a rough completeness check.
function countNodes(stmt: any): number {
    return 1; // shallow
}
const total = result.program.stmts.reduce((n, s) => n + countNodes(s), 0);
console.log(`\n(rough) total top-level nodes: ${total}`);
