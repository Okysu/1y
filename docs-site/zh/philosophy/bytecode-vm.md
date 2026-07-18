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

## VM 尚未支持的部分

少数 1y 特性仍仅 tree-walker 支持（VM 会回退到 tree-walker 或抛出明确错误）：

- `for` 循环（编译期为 stub）
- `break` / `continue`
- 字符串插值
- `try` / `transact`（进行中）
- `actor` 定义（actor 的 *spawn* 与消息传递可用；actor 主体编译尚不完整）

为最大化兼容性，tree-walker（`1y run`）仍然功能完整，作为参考实现。

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
