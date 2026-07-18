# 1y

**中文** | [English](./README.md)

> 一门用 Rust 实现的流式、并发、函数式编程语言。

`1y`（读作 "one-why"）拥有**两套执行后端**——树遍历解释器和基于栈的字节码
虚拟机——融合了持久化数据结构、模式匹配、Actor 并发模型、软件事务内存
（STM）、Zig 风格的无色异步以及实用的模块系统，全部由任意精度算术支撑。
同时还内置**运行时反射**（`ast_of`、`eval`、类型谓词），为正在进行的自举
工作奠定基础。

---

## 核心特性

- **双执行后端** — 字节码 VM（默认，`1y <file>`）的调用帧存于堆上，
  `fib_memo(100000)` 也不增长 Rust 栈；树遍历器（`1y run <file>`）用于
  调试与对比。
- **任意精度数值** — 整数和小数永远不会溢出。`fact(500)` 原生返回一个 1135 位的数字。
- **持久化集合** — `Vec`、`Map`、`Set` 均为不可变且结构共享，由 `im` crate 驱动。
- **模式匹配** — 字面量、绑定变量、通配符、Or 模式、向量/映射/结构体/变体解构，支持守卫。
- **Actor 并发** — 隔离状态、消息传递（`!` 发后即忘，`?` 请求/回复），Actor 之间无共享可变状态。
- **软件事务内存** — `shared` 单元 + `transact` 块，提供快照隔离、原子提交、回滚、嵌套和 `retry`。
- **无色异步（Zig 风格）** — `await` 可在任意函数中使用，无需 `async` 着色；
  corosensei stackful 协程驱动调度器，让 `accept_async` 与慢请求处理器并发推进。
- **模块系统** — `import` 标准库或自己的 `.1y` 文件；`lazy import` 延迟到首次访问时加载；循环引用检测。
- **反射与动态求值** — `eval(src)` 把字符串当 1y 程序执行，共享调用方的全局环境与类型表；
  `ast_of(src)` 把源码解析为 AST 数据；`type_of` / `instance_of` /
  `variant_name` / `variant_args` / `keys` / `values` / `fields` 组成完整反射面。
- **标准库** — `io`、`json`、`env`、`process`、`random`、`socket`（TCP）、
  `serial`（RS-232）、`crypto`（SHA/HMAC/base64/CSPRNG）、`tls`（rustls）、
  `ffi`（通过 `libloading` 动态加载库）。
- **异常** — `raise`、`try / rescue / ensure`，任何值都可以被抛出。
- **字符串插值** — `"hello {name}!"`，三引号多行字符串，`\{` 转义字面大括号。

---

## 快速开始

### 构建

```bash
cargo build --release
# 二进制文件：target/release/1y
```

### Hello World

```bash
1y -e 'println("Hello, World!")'          # VM（默认）
1y run -e 'println("Hello, World!")'      # 树遍历器
```

### 运行文件

```bash
1y examples/phase1.1y                     # VM（默认）
1y run examples/phase1.1y                 # 树遍历器
```

### REPL

```bash
1y repl
```

---

## 语言一览

```1y
// 函数是一等值。
fn fib(n) -> Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

// 任意精度整数 — 永不溢出。
let big = fact(500)

// 持久化集合。
let xs = [1, 2, 3]
let ys = push(xs, 4)      // xs 保持不变

// 带守卫和 Or 模式的模式匹配。
match opt {
    Some(x) if x > 0 => "positive",
    Some(_)          => "non-positive",
    None | Err(_)    => "nothing"
}

// 管道操作符，流式链式调用。
let result = xs |> filter(fn(x) { x > 0 }) |> map(double) |> sum

// 带隔离状态的 Actor。
actor Counter {
    state count = 0
    on inc()        { count = count + 1 }
    on get() -> Int { reply(count) }
}
let c = spawn Counter()
c ! inc()
let n = c ? get()

// 软件事务内存。
shared counter = 0
transact {
    counter = counter + 1
}

// 模块 — 标准库和你自己的 .1y 文件。
import io
import json
import utils.math as m       // 加载 <入口目录>/utils/math.1y
lazy import heavy_lib        // 首次使用时才加载

// 反射与动态求值。
let ast = ast_of("1 + 2");   // { "type": "Program", "stmts": [...] }
eval("let x = 10; x * 2");   // 20 — 定义会写入全局
enum EvError { Bad(String) }
let v = eval("Bad(\"boom\")");
variant_name(v);             // "Bad"
```

---

## 示例

| 文件 | 演示内容 |
|------|---------|
| `examples/phase1.1y` | 核心语言：函数、闭包、集合、模式匹配 |
| `examples/phase2.1y` | Actor 运行时：spawn、状态、发后即忘、KV 存储 |
| `examples/phase3.1y` | STM：shared 单元、transact、retry、嵌套 |
| `examples/phase4.1y` | 模块系统、标准库（io/json/random/process）、惰性导入 |
| `examples/phase4.5.1y` | 加密（SHA/HMAC/base64）、TLS 客户端、FFI 桩 |
| `examples/phase4.6.1y` | 真实 FFI：加载共享库、调用原生函数 |
| `examples/bench.1y` | 基准测试套件：fib、阶乘、循环、集合、JSON |
| `bootstrap/interp.1y` | **自举阶段 1**：用 1y 写的 1y tree-walker |
| `bootstrap/test_eval.1y` | `eval` / `ast_of` / 反射函数测试集 |

---

## 自举路线图

1y 的最终目标是托管自身的实现。反射内置函数（`ast_of`、`eval`、类型谓词）
是这项工作的基础。规划中的 5 阶段路径：

1. **✅ tree-walker in 1y** — [`bootstrap/interp.1y`](./bootstrap/interp.1y)
   用 1y 子集实现了 1y 子集的 tree-walker。阶段 1 已完成，能跑现有测试。
2. **⏳ parser in 1y** — 手写递归下降 parser，输出 `Vec` / `Map` 形式的
   AST（即 `ast_of` 的返回结构）。
3. **⏳ 字节码编译器 in 1y** — 把 AST 编译成 `Vec<Int>` 字节流。
4. **⏳ VM 解释循环 in 1y** — `match` 分发操作码。
5. **⏳ 替换 Rust VM** — 用 1y 实现的 VM 跑所有现有测试。

详见文档站 [反射与动态求值](https://okysu.github.io/1y/zh/syntax/introspection)。

---

## 工具链

### VSCode 扩展

`editor/` 目录包含一个 VSCode 扩展（`onely-vscode`），提供：

- **语法高亮** — `1y` 源码的 TextMate 语法。
- **LSP 诊断** — 混合策略：进程内 TS 词法分析器提供即时反馈 +
  可选的 `1y parse -` 子进程提供权威解析器错误。
- **上下文感知补全** — 关键字、内置函数、模块函数、用户定义符号。
- **悬停文档** — 关键字、内置函数和用户定义的函数/变量/类型。
- **文档符号** — `fn`、`let`、`enum`、`type`、`actor`、`on` 声明的大纲。
- **内联建议** — 常见构造的幽灵文本补全。

构建并安装：

```bash
cd editor
npm install
npm run package        # 生成 onely-vscode-0.1.0.vsix
code --install-extension onely-vscode-0.1.0.vsix
```

### 文档站点

`docs-site/` 目录包含一个 VitePress 文档站点，提供中英文双语内容，
涵盖设计哲学、语法参考和示例。

```bash
cd docs-site
npm install
npm run dev            # 本地预览 http://localhost:5173
npm run build          # 静态站点输出到 docs-site/.vitepress/dist
```

在线文档：https://okysu.github.io/1y/

---

## 项目结构

```
1y/
├── src/
│   ├── ast/            # AST 定义 + 位置信息 + to_value（AST → Value）
│   ├── lexer/          # 手写词法分析器
│   ├── parser/         # 递归下降解析器
│   ├── compiler/       # AST → 字节码 Chunk 编译器
│   ├── vm/             # 基于栈的字节码 VM（Vm + VmCtx）
│   ├── runtime/        # 协程调度器 + 跨线程 actor 注册表
│   ├── interpreter/    # 树遍历求值器
│   │   ├── builtins.rs # 内置函数
│   │   ├── env.rs      # 词法环境
│   │   ├── ops.rs      # 运算符
│   │   └── stdlib/     # 标准库模块
│   ├── value.rs        # 运行时值类型
│   ├── main.rs         # CLI 入口
│   └── lib.rs          # 库 API
├── tests/              # 集成测试（502 个测试）
├── examples/           # 示例程序
├── bootstrap/          # 自举工作（阶段 1：interp.1y）
├── editor/             # VSCode 扩展
├── docs-site/          # VitePress 文档
├── docs/               # 语言指南、标准库参考、架构文档
└── Cargo.toml
```

---

## 测试

```bash
cargo test              # 运行全部 502 个测试
cargo test -- --nocapture phase1   # 运行特定测试套件
```

---

## 许可证

双重许可：MIT 或 Apache-2.0，任选其一。
