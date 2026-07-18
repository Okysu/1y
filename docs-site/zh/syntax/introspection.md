---
title: 反射与动态求值
---

# 反射与动态求值

1y 内置一组**反射（introspection）函数**与 **`eval(src)` 动态求值**，可在运行时观察与构造值、解析源码、执行字符串形式的 1y 代码。这套能力是后续自举（在 1y 中实现 1y 解释器）的基础，也让构建 REPL、调试器、代码生成器变得可能。

## 类型谓词

每个内置类型都有对应的 `is_*` 谓词，返回 `Bool`：

| 函数 | 判定 |
|------|------|
| `is_int(v)` | `Int` |
| `is_decimal(v)` | `Decimal` |
| `is_str(v)` | `Str` |
| `is_bool(v)` | `Bool` |
| `is_nil(v)` | `Nil` |
| `is_vec(v)` | `Vec` |
| `is_map(v)` | `Map` |
| `is_set(v)` | `Set` |
| `is_number(v)` | `Int` 或 `Decimal` |
| `is_func(v)` | 任何可调用值（闭包 / 原生函数） |
| `is_closure(v)` | 用户定义闭包或 `fn` 字面量 |

```1y
is_int(42)           // true
is_decimal(3.14)     // true
is_str("hi")         // true
is_vec([1, 2, 3])    // true
is_closure(println)  // false —— println 是原生函数
is_closure(fn(x){x}) // true
is_func(println)     // true —— is_func 包含原生函数
```

## `type_of(v)` — 类型名

返回类型名字符串，与 `instance_of` 的第二参数一致（详见下面的规范化规则）。

```1y
type_of(42)              // "Int"
type_of("hi")            // "String"
type_of([1, 2])          // "Vec"
type_of(fn(x){x})        // "Closure"
type_of(println)         // "Native"
type_of(Some(42))        // "Variant"
```

## `instance_of(v, type_name)` — 类型判定

判断 `v` 是否属于给定类型名。对 `Variant` / `Struct` 比较构造器名；对其他类型比较规范化后的类型名。

**类型名规范化**：`type_of` 返回 `"String"`，但谓词族用 `"Str"`；`instance_of` 内部把它们统一化，所以 `"Str"` 与 `"String"` 都能识别字符串。同理 `"Func"` 与 `"Closure"` 也互通。

```1y
enum Option { Some(v), None }
let s = Some(42)

instance_of(s, "Some")       // true
instance_of(s, "Option")     // false —— 比较的是 variant 名而非 enum 名
instance_of(s, "None")       // false
instance_of(42, "Int")       // true
instance_of("hi", "Str")     // true
instance_of("hi", "String")  // true —— 规范化后两者等价
instance_of(fn(x){x}, "Closure") // true
instance_of(fn(x){x}, "Func")    // true
```

## 集合与结构体内省

| 函数 | 输入 | 输出 |
|------|------|------|
| `keys(map_or_struct)` | `Map` / `Struct` | 字段名的 `Vec` |
| `values(map_or_struct)` | `Map` / `Struct` | 字段值的 `Vec` |
| `fields(struct)` | `Struct` | `Map`（字段名 → 值） |
| `has_key(map, key)` | `Map` | `Bool` |
| `count(coll)` | `Vec` / `Map` / `Set` / `Str` | `Int` |

```1y
let m = { "a": 1, "b": 2 };
keys(m)     // ["a", "b"]（或 ["b", "a"]，哈希序）
values(m)   // [1, 2]
has_key(m, "a") // true

type Point = { x: Int, y: Int };
let p = Point({ x: 10, y: 20 });
keys(p)     // ["x", "y"]
values(p)   // [10, 20]
fields(p)   // { "x": 10, "y": 20 }
```

## Variant 内省

| 函数 | 输入 | 输出 |
|------|------|------|
| `variant_name(v)` | `Variant` | 构造器名 `Str` |
| `variant_args(v)` | `Variant` | 携带参数的 `Vec` |

```1y
enum Tree { Leaf, Node(v, l, r) }

let t = Node(42, Leaf, Leaf);
variant_name(t)    // "Node"
variant_args(t)    // [42, Leaf, Leaf]

let l = Leaf;
variant_name(l)    // "Leaf"
variant_args(l)    // []
```

## `ast_of(src)` — 解析源码为 AST

把 1y 源码字符串解析成 AST，返回嵌套的 `Map` 结构（每个节点形如 `{ "type": "NodeType", ...fields }`）。解析失败时返回一个 `ParseError` 结构而非抛异常——便于程序化处理。

```1y
let ast = ast_of("let x = 1 + 2; fn add(a, b) \{ a + b \}");
get(ast, "type")           // "Program"
count(get(ast, "stmts"))   // 2

let s0 = get(get(ast, "stmts"), 0);
get(s0, "type")            // "Let"
get(s0, "name")            // "x"

let val = get(s0, "value");
get(val, "type")           // "BinOp"
get(val, "op")             // "+"
get(get(val, "lhs"), "value")  // 1
get(get(val, "rhs"), "value")  // 2

let s1 = get(get(ast, "stmts"), 1);
get(s1, "type")            // "FuncDef"
get(s1, "name")            // "add"
count(get(s1, "params"))   // 2
```

### 解析错误结构

```1y
let bad = ast_of("let x = ;");
// bad == {
//   "type": "ParseError",
//   "message": "unexpected `;` in expression",
//   "line": 1,
//   "col": 9,
// }

get(bad, "type")     // "ParseError"
get(bad, "message")  // "unexpected `;` in expression"
get(bad, "line")     // 1
get(bad, "col")      // 9
```

### AST 节点形态

常见节点结构：

| 节点 type | 关键字段 |
|-----------|---------|
| `Program` | `stmts: [Stmt]` |
| `Let` | `name: Str`, `value: Expr`, `is_rec: Bool` |
| `FuncDef` | `name: Str`, `params: [Param]`, `body: Expr` |
| `Expr` / `Semi` | `expr: Expr` |
| `BinOp` | `op: Str`, `lhs: Expr`, `rhs: Expr` |
| `UnaryOp` | `op: Str`, `operand: Expr` |
| `If` | `cond: Expr`, `then: Expr`, `else: Option<Expr>` |
| `Call` | `callee: Expr`, `args: [Expr]` |
| `Ident` | `name: Str` |
| `Int` / `Decimal` / `Bool` | `value: <同类型>` |
| `Str` | `value: Str`（单部分）或 `parts: [StrPart]`（含插值） |

> 完整定义见 [`src/ast/to_value.rs`](https://github.com/Okysu/1y/blob/main/src/ast/to_value.rs)。

## `eval(src)` — 动态求值

把字符串作为 1y 程序解析、编译、执行。**与外层共享全局环境与类型表**——`eval` 中定义的 `fn` / `let` 会写入全局，`eval` 中也能引用外层已定义的 `enum` variant 与 `type` 构造器。

返回最后一个**表达式**的值；若最后是语句（`let` / `fn` 定义等），返回 `Nil`。

```1y
// 简单表达式
eval("1 + 2 * 3")              // 7
eval("\"hello\" + \" world\"") // "hello world"

// 多语句程序：最后表达式的值作为结果
eval("let x = 10; let y = 20; x + y")  // 30

// 定义持久化：写入全局
eval("fn sq(n) \{ n * n \}");
sq(5)                          // 25 —— 外层可直接调用

// 引用外层全局
let base = 100;
eval("base + 1")               // 101

// 闭包
eval("fn mk(n) \{ fn(x) \{ x + n \} \}; mk(10)(5)")  // 15

// 嵌套 eval
eval("eval(\"1 + 2\") + 1")    // 4
```

### 识别外层类型

`eval` 默认能看到外层的 `enum` variant 与 `type` 构造器：

```1y
enum EvError { Bad(String), NoArgs, WithTwo(Int, Int) }
type EvPoint = \{ x: Int, y: Int \};

let v = eval("Bad(\"from eval\")");
variant_name(v)                // "Bad"
variant_args(v)                // ["from eval"]

let z = eval("NoArgs");
variant_name(z)                // "NoArgs"

let p = eval("EvPoint(\{ x: 10, y: 20 \})");
instance_of(p, "EvPoint")      // true
fields(p)                      // { "x": 10, "y": 20 }
```

### 异常传播

`eval` 内的 `raise` 会向外传播，可被外层 `try` / `rescue` 捕获：

```1y
try {
    eval("raise(\"boom from eval\")");
} rescue as e {
    println("caught: " + str(e));  // caught: boom from eval
}

// variant 异常也能传播
enum Err { Bad(String) }
try {
    eval("raise(Bad(\"nested\"))");
} rescue as e {
    variant_name(e);    // "Bad"
    variant_args(e);    // ["nested"]
}
```

> **注意**：`try` / `rescue` 只捕获 `raise` 抛出的 `UserException`，**不**捕获类型错误、解析错误等运行时错误——这与 tree-walker 语义一致。若需先检查源码是否合法，配合 `ast_of` 使用。

### 空程序

`eval("")` 返回 `Nil`，不报错。

## 限制

当前 `eval` 与反射能力的已知限制：

- **不支持跨 `import` 边界的 variant 共享**：模块里定义的 `enum` variant 在 `eval` 字符串中可以引用（因为 VM 维护了持久化类型表），但**跨 actor 边界**的 variant 比较依赖值本身的 `name` 字段，没有命名空间冲突检查。
- **`ast_of` 输出是约定而非稳定 API**：AST 节点的具体字段名可能随语法演进调整；程序化处理时请优先用 `get(ast, "type")` 分派。
- **`eval` 不是沙箱**：eval'd 代码与外层共享全局与文件系统访问，不要用于执行不受信任的源码。

## 自举路径

这套能力是 1y 自举的根基。规划中的 5 阶段自举路径：

1. ✅ **tree-walker in 1y**（`bootstrap/interp.1y`）——用 1y 子集实现 1y 子集的 tree-walker，证明自解释可行。
2. ⏳ **parser in 1y** —— 手写递归下降 parser，输出 `Vec` / `Map` 形式的 AST（即 `ast_of` 的返回结构）。
3. ⏳ **字节码编译器 in 1y** —— 把 AST 编译成 `Vec<Int>` 字节流。
4. ⏳ **VM 解释循环 in 1y** —— `match` 分发操作码。
5. ⏳ **替换 Rust VM** —— 用 1y 实现的 VM 跑所有现有测试。

阶段 1 已完成；阶段 2-5 待办。详见 [字节码虚拟机](../philosophy/bytecode-vm)。

## 参考

- [标准库概览](./stdlib) —— 全部内置函数索引
- [字节码虚拟机](../philosophy/bytecode-vm) —— VM 实现细节，包括 `eval` 的执行模型
- [`src/ast/to_value.rs`](https://github.com/Okysu/1y/blob/main/src/ast/to_value.rs) —— AST → `Value` 的转换代码（`ast_of` 的实现）
- [`src/vm/vm.rs`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) 中的 `VmCtx::eval_src` —— `eval` 的 VM 端实现
