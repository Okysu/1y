// Verify the workspace index: module path → file URI resolution, cross-file
// symbol lookup, and workspace symbol search. Run from the editor dir with:
//   npx ts-node scripts/test-workspace.ts <workspace-root>
import { buildWorkspaceIndex, resolveModuleUri, findSymbolInFile, searchWorkspaceSymbols } from "../src/workspaceIndex";

const root = process.argv[2] || "../examples";
const index = buildWorkspaceIndex([require("path").resolve(root)]);

console.log(`=== workspace index for ${root} ===`);
console.log(`files: ${index.files.size}`);
for (const [uri] of index.files) console.log(`  ${uri}`);
console.log(`symbols: ${index.symbols.length}`);

console.log("\n-- module path resolution --");
for (const mod of ["lib.yin", "lib.http", "json", "io", "yin", "http"]) {
    const u = resolveModuleUri(index, mod);
    console.log(`  ${mod} → ${u || "<stdlib/none>"}`);
}

console.log("\n-- cross-file symbol lookup --");
const yinUri = resolveModuleUri(index, "lib.yin");
if (yinUri) {
    for (const name of ["new", "group", "register", "match_route", "split_path"]) {
        const sym = findSymbolInFile(index, yinUri, name);
        console.log(`  lib.yin :: ${name} → ${sym ? `${sym.kind} @${sym.nameSpan.start}` : "<not found>"}`);
    }
}

console.log("\n-- workspace symbol search 'route' --");
for (const r of searchWorkspaceSymbols(index, "route")) {
    console.log(`  ${r.name} [${r.kind}] in ${r.uri.split("/").pop()}`);
}

console.log("\n-- workspace symbol search 'new' --");
for (const r of searchWorkspaceSymbols(index, "new").slice(0, 8)) {
    console.log(`  ${r.name} [${r.kind}] in ${r.uri.split("/").pop()}`);
}
