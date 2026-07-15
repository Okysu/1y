# 1y

**中文** | [English](./README.md)

> 一门用 Rust 实现的流式、并发、函数式编程语言。

`1y`（读作 "one-why"）是一门树遍历解释型语言，融合了持久化数据结构、模式匹配、
Actor 并发模型、软件事务内存（STM）以及实用的模块系统——全部由任意精度算术支撑。

---

## 核心特性

- **任意精度数值** — 整数和小数永远不会溢出。`fact(500)` 原生返回一个 1135 位的数字。
- **持久化集合** — `Vec`、`Map`、`Set` 均为不可变且结构共享，由 `im` crate 驱动。
- **模式匹配** — 字面量、绑定变量、通配符、Or 模式、向量/映射/结构体/变体解构，支持守卫。
- **Actor 并发** — 隔离状态、消息传递（`!` 发后即忘，`?` 请求/回复），Actor 之间无共享可变状态。
- **软件事务内存** — `shared` 单元 + `transact` 块，提供快照隔离、原子提交、回滚、嵌套和 `retry`。
- **模块系统** — `import` 标准库或自己的 `.1y` 文件；`lazy import` 延迟到首次访问时加载；循环引用检测。
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
1y run -e 'println("Hello, World!")'
```

### 运行文件

```bash
1y run examples/phase1.1y
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
│   ├── ast/            # AST 定义 + 位置信息
│   ├── lexer/          # 手写词法分析器
│   ├── parser/         # 递归下降解析器
│   ├── interpreter/    # 树遍历求值器
│   │   ├── builtins.rs # 内置函数
│   │   ├── env.rs      # 词法环境
│   │   ├── ops.rs      # 运算符
│   │   └── stdlib/     # 标准库模块
│   ├── value.rs        # 运行时值类型
│   ├── main.rs         # CLI 入口
│   └── lib.rs          # 库 API
├── tests/              # 集成测试（410 个测试）
├── examples/           # 示例程序
├── editor/             # VSCode 扩展
├── docs-site/          # VitePress 文档
├── docs/               # 语言指南、标准库参考、架构文档
└── Cargo.toml
```

---

## 测试

```bash
cargo test              # 运行全部 410 个测试
cargo test -- --nocapture phase1   # 运行特定测试套件
```

---

## 许可证

双重许可：MIT 或 Apache-2.0，任选其一。
