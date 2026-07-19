---
title: 性能基准测试
---

# 性能基准测试

使用 `tests/bench_yin.py` 对 yin Web 框架进行并发性能测试。

## 运行基准测试

```bash
# 终端 1：启动服务器（VM 后端，默认）
cargo run --release -- examples/yin_bench_server.1y

# 终端 2：运行压测
python tests/bench_yin.py
```

如需对比传统的 tree-walker，用 `run` 启动：

```bash
cargo run --release -- run examples/yin_bench_server.1y
```

## 测试结果

测试环境：Windows，release 构建，单线程事件循环。

### VM（默认后端）—— 并发性能（GET /ping）

| 请求数 | 顺序 (ms) | 并发 (ms) | 顺序 (req/s) | 并发 (req/s) | 加速比 | 成功率 |
|-------:|----------:|----------:|-------------:|-------------:|-------:|-------:|
| 10     |      87.1 |      11.6 |        114.8 |        863.1 | 7.52x  | 10/10  |
| 100    |     292.7 |      65.4 |        341.7 |       1529.2 | 4.48x  | 100/100 |
| 1000   |    3941.8 |     608.1 |        253.7 |       1644.4 | 6.48x  | 1000/1000 |
| 10000  |   43168.7 |    5510.5 |        231.6 |       1814.7 | 7.83x  | 10000/10000 |

### Tree-walker（`1y run`）—— 并发性能（GET /ping）

| 请求数 | 顺序 (ms) | 并发 (ms) | 顺序 (req/s) | 并发 (req/s) | 加速比 | 成功率 |
|-------:|----------:|----------:|-------------:|-------------:|-------:|-------:|
| 10     |      57.8 |      18.0 |        172.9 |        556.0 | 3.21x  | 10/10  |
| 100    |     332.3 |      99.0 |        301.0 |       1010.1 | 3.36x  | 100/100 |
| 1000   |    4518.5 |     595.1 |        221.3 |        808.0 | 7.59x  | 1000/1000 |
| 10000  |   55618.1 |    8279.9 |        179.8 |        671.8 | 6.72x  | 10000/10000 |

### VM vs Tree-walker（10000 并发）

| 后端 | 吞吐 | 相对 TW |
|------|-----:|--------:|
| Tree-walker | 672 req/s | 1.00x |
| **字节码 VM** | **1815 req/s** | **2.70x** |

VM 在 10000 并发下实现 **2.7 倍吞吐提升**，顺序吞吐提升 **85%**（232 vs 180 req/s）。两端 10000/10000 请求全部成功。

### 优化历程

| 版本 | 10000 并发吞吐 | 10000 成功率 |
|------|---------------:|-------------:|
| 原始版（sleep_ms 轮询，tree-walker） | 279 req/s | 1.6% |
| accept_async（mio 事件驱动，tree-walker） | 534 req/s | 99.96% |
| 多线程（N-worker + 批量 accept，tree-walker） | 1547 req/s | 76.9% |
| 单线程 VM + 调度器调优 | 1815 req/s | 100% |

### 无色异步验证

发送 1 个慢请求（GET /slow，`await process.sleep_async(500)`）+ 5 个快请求（GET /ping）并发：

| 标签 | 耗时 (ms) | 状态 |
|------|----------:|-----:|
| SLOW |     518.2 |  200 |
| fast0 |       4.0 |  200 |
| fast1 |       4.0 |  200 |
| fast2 |       3.8 |  200 |
| fast3 |       3.7 |  200 |
| fast4 |     517.2 |  200 |

5 个快请求中有 4 个在 ~4 ms 内完成，而慢处理器仍在等待其 500 ms 定时器——验证了 `await process.sleep_async` **不会**阻塞事件循环。慢请求本身在 518 ms 正常完成。原理见 [无色异步](../syntax/async) 与 [字节码 VM](../philosophy/bytecode-vm)。

## 优化技术

### 1. accept_async（mio 事件驱动 I/O）

`socket.accept_async(listener)` 将监听器注册到 `mio`（epoll/kqueue/IOCP）。调度器仅在有连接待处理时被唤醒——不会空转。

### 2. 定时器最小堆

`process.sleep_async(ms)` 会在 Task 上盖一个 deadline 提示。调度器把等待定时器的协程放入按 deadline 排序的 `BinaryHeap`，于是：

- `mio::poll` 精确 sleep 到下一个定时器触发（或 I/O 事件先到，取早者）——不再 1 ms 空轮询。
- 每次 tick 的定时器 push/pop 是 O(log n)，而非 O(n) 扫描所有 parked task。

### 3. 有界 `yield` tick

每次 `yield` 最多推进调度器 4 个 tick 就把控制权交还 accept loop。这让慢处理器取得进展（其定时器会在一次 `yield` 内触发），又不会把 accept loop 困满整个等待时长。快处理器（读 → 跑 → 写，各 yield 一次）仍能在一次 `yield` 内完成。

### 4. top-level `await` 推进调度器

HTTP accept loop 是 top-level 代码：其 `await accept_async` 不在任何协程内运行。VM 的 top-level `await` 在 busy-poll 自身 Task 的**同时，在轮询之间穿插 `drain_mailboxes_async` tick**，让 parked 协程（定时器、I/O）继续推进。否则 top-level await 会饿死所有 parked 协程——例如慢处理器的 500 ms 定时器永远不会触发。

### 5. actor 消息发送时关闭 upvalue

当 `!`（ActorSend）把包含闭包的消息入队时，VM 会立即关闭该闭包（以及任何嵌套值中）的所有 open upvalue。这防止接收方协程在发送方（现已独立）的 `VmCtx` 里读到悬空栈槽。

## 后续工作

- VM 的尾调用优化，以支持更深递归
- 多线程 VM（当前单线程；tree-walker 的 `parallel` 模块尚未 VM 化）
- 字节类型与字节缓冲（自托管 VM 当前用 `Vec<Int>` 表示字节码）

> **已完成**：用 1y 自举实现 1y VM —— 见 [字节码 VM](../philosophy/bytecode-vm#自举)
> 与 `bootstrap/selfvm.1y`。`1y selfvm <file.1y>` 现在能跑完整地用 1y
> 实现的 lex → parse → compile → VM 流水线。
