---
title: 字节码虚拟机
---

# 字节码虚拟机

1y 最初采用 tree-walking 解释器，现已搭载基于栈的字节码虚拟机（VM）。VM 会将 AST 编译为扁平的指令序列，在分发循环中执行。VM 是默认后端（`1y <file>`）；传统的 tree-walker 仍可通过 `1y run <file>` 使用，便于调试与对比。

## 为什么需要 VM？

原始的 tree-walking 解释器递归求值 AST。每求值一个表达式都会分配新的 `EnvRef`，并逐节点遍历 AST、穿过层层嵌套的 `match` 分支。这种方式简单但缓慢：每层递归约占用 5–10 KB 栈，且每个节点都有分发开销。

字节码 VM 同时解决这两个问题：

- **扁平分发循环** —— 操作码在 `VmCtx::step` 的紧凑 `match` 中解码，无需遍历 AST。
- **调用帧存于堆** —— VM 栈是堆上的 `Vec<Value>`，所以 `fib_memo(10000)` 这样的深递归不再溢出 Rust 栈。
- **指令复用** —— 编译后的闭包共享其 `Chunk`（操作码缓冲），定义一次的函数反复调用代价很低。

## 架构

```
┌─────────────────────────────┐
│            Vm               │  共享全局状态
│  ─ global env               │  (imports、类型注册表、活跃 actors)
│  ─ variants / structs       │
│  ─ live_actors              │
│  ─ scheduler                │
│  ─ module cache             │
└────────────┬────────────────┘
             │ 每个 handler spawn 一个
             ▼
┌─────────────────────────────┐
│          VmCtx              │  协程级执行上下文
│  ─ stack: Vec<Value>        │  操作数 + 调用栈
│  ─ frames: Vec<Frame>       │  调用帧（返回 PC、局部基址）
│  ─ open_upvalues: Vec<...>  │  Lua 风格 open upvalue
│  ─ chunks: Vec<Rc<Chunk>>   │  字节码 chunk（共享）
└─────────────────────────────┘
```

`Vm`（共享、长生命周期）与 `VmCtx`（每个 handler 独立、短生命周期）的拆分呼应 Actor 模型：每条 `Handle` 消息 spawn 一个新的 `VmCtx`，在共享 `Vm` 的调度器上运行至完成（或挂起）。

### 编译

`Compiler` 对每个函数遍历一次 AST，产出 `Chunk`：

```
fn fib(n) {
    if n < 2 { return n };
    fib(n - 1) + fib(n - 2)
}
```

大致编译为：

```
LoadLocal 0      ; 压入 n
PushInt 2
Less
JumpIfFalse L1
LoadLocal 0      ; return n
Return
L1:
LoadGlobal "fib" ; fib(n - 1)
LoadLocal 0
PushInt 1
Sub
Call 1
LoadGlobal "fib" ; fib(n - 2)
LoadLocal 0
PushInt 2
Sub
Call 1
Add
Return
```

### 操作数栈与调用帧

VM 基于栈。每次 `Call` 压入一个新 `Frame`，记录返回 PC 和局部基址；`Return` 弹出。局部变量直接存于栈上（无独立的局部数组），因此 `LoadLocal 0` 就是一次数组索引。

### 闭包与 Upvalue

闭包从外层作用域捕获 upvalue，采用 Lua 的设计：

- **Open upvalue** —— 指向仍然活跃的栈槽。
- **Closed upvalue** —— 栈槽已被弹出；值被移到堆上，闭包仍可访问。

当闭包逃逸到另一个协程时（例如通过 actor 消息 `!`），`close_escaping_upvalues` 会立即关闭其所有 open upvalue，防止接收方在自己（独立）的栈里读到垃圾数据。

## 已支持的操作码

VM 实现了 1y 核心语义所需的全部操作码：

| 类别 | 操作码 |
|------|--------|
| 字面量 | `PushNil`、`PushInt`、`PushDecimal`、`PushStr`、`PushBool` |
| 变量 | `LoadLocal`、`StoreLocal`、`LoadGlobal`、`StoreGlobal`、`LoadUpvalue`、`StoreUpvalue` |
| 集合 | `NewVec`、`NewMap`、`GetField`、`SetField`、`GetIndex`、`SetIndex` |
| 控制流 | `Jump`、`JumpIfFalse`、`JumpIfTrue`、`Return` |
| 调用 | `Call`、`TailCall`（有限） |
| 函数 | `Closure`、`PopLocalKeep` |
| Actor | `Spawn`、`ActorSend` (`!`)、`ActorCall` (`?`)、`ActorReply` |
| 异步 | `Await`、`Yield` |
| 模块 | `Import`、`GetMember` |
| 模式匹配 | `Match`、`TestTag`、`TestLiteral`、`Bind`、`JumpIfNoMatch` |
| 结构体/枚举 | `NewStruct`、`NewVariant`、`GetVariantArgs` |

## 异步与调度器

VM 复用与 tree-walker 相同的 `Scheduler`。`await` 编译为 `OpCode::Await`：

- **协程内** —— `await_task` 读取 thread-local 的 `CURRENT_YIELDER`，通过 `yielder.suspend()` 挂起。调度器 park 当前协程并运行其他协程。
- **top-level fallback** —— 不在协程内时（例如 HTTP accept loop 的 `await accept_async`），VM 会 busy-poll Task，但**在轮询之间推进调度器**，让 parked 协程（定时器、I/O）继续取得进展而不被饿死。

正是这一点让 yin Web 服务器在高负载下保持并发：accept loop 是普通的 `loop { await accept_async; yield }`，其 top-level `await` 把调度器 tick 与自身的轮询交错，使慢处理器的定时器在 accept loop 等待新连接时就能触发。

用户侧 API 见 [无色异步](../syntax/async)。

## 栈溢出修复

tree-walker 的 `fib_memo(10000)` 会溢出 256 MB 的 Rust 栈，因为每层递归约占 10 KB。VM 通过把调用帧搬到堆上（`Vec<Frame>`）解决——唯一的 Rust 栈帧就是 `VmCtx::step` 分发循环本身，无论 1y 侧递归多深都是有界的。

基准：`fib_memo(10000)` 在 VM 中运行无栈增长；`fib_memo(100000)` 约 1 秒完成。

## 控制信号与 handler 栈

VM 有 6 种控制信号：`Break` / `Continue` / `Retry` / `UserException` / `Reply` / `Return`。其中前四种需要找到对应的 handler（loop / transact / exception）才能消费。

最初实现用 `stack_depth >= cur_frame_base` 来匹配 handler，但这有一个隐患：子 frame 的 `stack_base` 可能与父 frame 的 handler `stack_depth` 相等，导致父 frame 的 handler 被错误匹配——`try { fn_that_raises() } rescue { ... }` 会让 rescue 跳到错误的位置，IP 越界 panic。

修复方式是给三类 handler 栈都加上 `frame_depth` 字段（`frames.len()`），改用精确匹配 `handler.frame_depth == cur_frame_depth`：

- [`ExceptionHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `try`/`rescue`/`ensure`
- [`TransactHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `transact`/`retry`
- [`LoopHandler`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) — `for`/`break`/`continue`

三处的信号分派统一走 `VmCtx::handle_signal(err, propagate_depth)`，返回 `SignalOutcome::Continue`（信号已消费）或 `SignalOutcome::Done(value)`（`Reply`/`Return` 已命中目标）。

## 动态求值（eval）

VM 端的 `eval(src)` 实现是 [`VmCtx::eval_src`](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs)。流程：

1. 解析源码（`crate::parser::parse`）
2. 用 `compile_program_with_types` 编译为 chunk——这一步会注入 VM 持久化的 `variant_table` / `struct_table`，所以 eval'd 代码能识别外层 `enum` variant 与 `type` 构造器
3. 在当前 `VmCtx` 上 push 一个子 `Frame` 运行 chunk（共享 `vm.globals`）
4. 步进到子 frame 返回，期间通过 `handle_signal(e, target_depth)` 路由信号——`raise` 能被外层 `try` 捕获

**持久化类型表**是 `Vm` 上的 `variant_table: HashMap<String, usize>` 和 `struct_table: HashMap<String, ()>` 字段。`run_source` / `load_module` / `eval_src` 三处编译入口都共享它们，所以 `enum`/`type` 定义会跨 `eval` / `import` 边界持续可见。

详见 [反射与动态求值](../syntax/introspection)。

## VM 已支持的全部特性

VM 已与 tree-walker 功能对齐——所有 1y 语言特性都在字节码后端中得到原生支持，无需回退：

- **控制流**：`if`/`match`/`while`/`loop`/`for`/`break`/`continue`/`return`
- **字符串插值**：`"x = {x}"` 编译为 `to_str` + `+` 链
- **模式匹配**：字面量、绑定、`Variant`/`Struct`/`Vec` 解构、Or 模式、守卫
- **闭包与 upvalue**：Lua 风格 open/closed upvalue，actor 消息发送时递归关闭逃逸闭包
- **异常**：`try`/`rescue`/`ensure`/`raise`，基于 `PushTry`/`PopTry`/`RescueMatch`/`EnsureExit` 操作码
- **软件事务内存**：`transact`/`retry`，基于 `PushTransact`/`TransactCommit`，含冲突检测与重试
- **Actor 并发**：`actor` 定义、`spawn`、`!`（send）、`?`（request）、`reply`、`yield`
- **无色异步**：`await` 基于 corosensei stackful 协程；top-level await 通过 `drain_mailboxes_async` 推进调度器
- **模块系统**：`import path [as alias] [lazy]`，按需加载并缓存
- **反射与 eval**：`ast_of` / `eval` / `type_of` / `instance_of` / `variant_name` / `variant_args` 等内置函数

502 个测试全部通过，覆盖以上所有特性。tree-walker（`1y run`）保留作为参考实现，方便对比与调试。

## 试一试

```bash
# VM（默认）
1y examples/fibonacci.1y

# Tree-walker（用于对比 / 调试）
1y run examples/fibonacci.1y

# 运行 VM 测试套件
1y vm tests/vm_test.1y
```

## 实现

- [src/compiler/mod.rs](https://github.com/Okysu/1y/blob/main/src/compiler/mod.rs) —— AST → Chunk 编译器
- [src/vm/vm.rs](https://github.com/Okysu/1y/blob/main/src/vm/vm.rs) —— `Vm` + `VmCtx` 执行引擎
- [src/runtime/scheduler.rs](https://github.com/Okysu/1y/blob/main/src/runtime/scheduler.rs) —— 协程调度器（与 tree-walker 共用）
