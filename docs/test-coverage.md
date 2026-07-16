# `1y` Test Coverage Report

Generated with `cargo-tarpaulin` on Phase 4.6 (coverage lines); test counts
updated through Phase C + yin.

## Summary

- **Total lines**: 5111
- **Covered lines**: 3257
- **Coverage**: 63.73%
- **Total tests**: 502 (Phase C + yin + parallel: +86 tests since Phase 4.6)

## Per-File Coverage

| File | Covered / Total | Notes |
|------|-----------------|-------|
| `src/ast/mod.rs` | 59 / 72 | AST node constructors well-covered |
| `src/ast/span.rs` | 29 / 33 | |
| `src/error.rs` | 19 / 38 | Error formatting paths |
| `src/interpreter/builtins.rs` | 249 / 371 | Some rarely-hit branches |
| `src/interpreter/env.rs` | 26 / 29 | |
| `src/interpreter/error.rs` | 32 / 47 | |
| `src/interpreter/mod.rs` | 777 / 939 | Core evaluator well-covered |
| `src/interpreter/ops.rs` | 95 / 184 | Some ops paths |
| `src/interpreter/stdlib/crypto.rs` | 101 / 132 | |
| `src/interpreter/stdlib/env.rs` | 32 / 42 | |
| `src/interpreter/stdlib/ffi.rs` | 113 / 231 | Arity dispatch branches |
| `src/interpreter/stdlib/io.rs` | 29 / 57 | Error paths |
| `src/interpreter/stdlib/json.rs` | 164 / 275 | Edge cases |
| `src/interpreter/stdlib/mod.rs` | 21 / 21 | Fully covered |
| `src/interpreter/stdlib/process.rs` | 42 / 73 | |
| `src/interpreter/stdlib/random.rs` | 67 / 93 | |
| `src/interpreter/stdlib/serial.rs` | 24 / 126 | Hardware-dependent |
| `src/interpreter/stdlib/socket.rs` | 18 / 130 | Network-dependent |
| `src/interpreter/stdlib/tls.rs` | 37 / 130 | Network-dependent |
| `src/lexer/mod.rs` | 299 / 396 | |
| `src/lexer/token.rs` | 46 / 125 | Keyword table |
| `src/main.rs` | 0 / 70 | CLI entry, not unit-tested |
| `src/parser/mod.rs` | 823 / 932 | |
| `src/printer.rs` | 102 / 365 | Pretty-printer (only roundtrip tested) |
| `src/value.rs` | 53 / 200 | Display/PartialEq branches |

## Analysis

### Well-covered areas (>80%)
- **Interpreter core** (`mod.rs` 82%): the evaluator's main paths are exercised
  by 51 interpreter tests + higher-order/loops/math/string tests.
- **Parser** (`parser/mod.rs` 88%): 41 parser tests + 6 roundtrip tests.
- **Lexer** (`lexer/mod.rs` 75%): 28 lexer tests.
- **Module system** (`stdlib/mod.rs` 100%): 17 module tests.
- **STM** (in `mod.rs`): 24 transact tests cover snapshot isolation, retry,
  nesting, commit/rollback.

### Under-covered areas (<50%)
- **`main.rs` (0%)**: CLI entry point is not unit-tested. Integration tested
  via example runs (`cargo run -- run examples/*.1y`).
- **`printer.rs` (28%)**: the AST pretty-printer is only exercised via
  roundtrip tests. Most node-type branches are not hit.
- **`value.rs` (27%)**: `Display`, `PartialEq`, and `Hash` implementations have
  many branches for types not commonly printed/compared in tests.
- **Network/hardware modules** (`socket.rs` 14%, `tls.rs` 28%, `serial.rs` 19%):
  these require live network/serial hardware, so only import and error-path
  tests run in CI.
- **`ffi.rs` (49%)**: the arity-dispatch has 7 branches (0–6 args); only a few
  are exercised by the libc tests.

### Test Suite Breakdown

| Test file | Count | Area |
|-----------|-------|------|
| `lexer_test.rs` | 28 | Tokenization |
| `parser_test.rs` | 41 | Parsing |
| `roundtrip_test.rs` | 6 | Parse→print→parse |
| `interpreter_test.rs` | 51 | Core evaluator |
| `higher_order_test.rs` | 40 | map/filter/fold |
| `loops_test.rs` | 31 | while/loop/break |
| `math_test.rs` | 42 | Math builtins |
| `string_test.rs` | 42 | String builtins |
| `actor_test.rs` | 21 | Actor runtime |
| `transact_test.rs` | 24 | STM |
| `module_test.rs` | 17 | Module system |
| `stdlib_test.rs` | 73 | Standard library |
| **Total** | **416** | |

## How to Regenerate

```sh
cargo tarpaulin --skip-clean --out Html --output-dir coverage
# or for console output:
cargo tarpaulin --skip-clean
```

The HTML report is at `coverage/tarpaulin-report.html`.
