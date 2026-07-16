---
title: 无色异步
---

# 无色异步 (Colorless Async)

1y 实现了 **Zig 风格的无色异步**：任何 `fn` 都可以使用 `await`，无需 `async` 关键字。不存在函数染色——同步和异步调用使用相同的调用约定。

## 创建 Task

`Task` 由可能阻塞的 I/O 操作产生：

```1y
import socket;
import process;

// socket.read_async — 在 WouldBlock 时挂起，有数据时恢复
let data = await socket.read_async(stream, 65536);

// process.sleep_async — 挂起指定毫秒数
await process.sleep_async(500);
```

## Task 组合子

```1y
// task_ready(value) — 立即就绪的 Task
let t1 = task_ready(42);

// task_all([t1, t2, ...]) — 所有 Task 完成时完成
let results = await task_all([t1, t2]);

// task_any([t1, t2, ...]) — 任一 Task 完成时完成
let first = await task_any([t1, t2]);
```

## 为什么没有 `async` 关键字？

在有函数染色的语言中（Python 的 `async def`、Rust 的 `async fn`、JS 的 `async`），必须标注可能 await 的函数，调用者也必须以不同方式处理 `Future`/`Promise`。这创造了两个世界：同步和异步，它们不能自由组合。

1y 使用 **stackful 协程**（`corosensei`）：`await` 挂起整个调用栈，因此任何函数——即使是不知道异步的函数——都可以从 `await` 上下文中调用，任何函数都可以开始 `await` 而无需改变签名。

## HTTP 处理器示例

```1y
import lib.http as http;

// 这个处理器只是一个普通 fn — 没有 `async` 标记。
// 它可以在内部 `await`，慢处理器不会阻塞其他连接。
fn handler(req) {
    await process.sleep_async(100);  // 模拟慢操作
    { "status": 200, "body": "done", "headers": [] }
}

http.serve("127.0.0.1:8080", handler)
```

## 事件驱动 I/O

1y 的调度器使用 `mio`（Linux: `epoll`，macOS: `kqueue`，Windows: IOCP）实现事件驱动 I/O：

- `socket.accept_async(listener)` — 挂起直到有连接待处理
- `socket.read_async(stream, n)` — 挂起直到有数据可读

当协程 await 时，调度器运行其他就绪的协程。慢处理器不会阻塞其他连接。

## 底层原理

1. **stackful 协程**（`corosensei`）：`await` 挂起整个调用栈
2. **协作式调度器**（`src/runtime/scheduler.rs`）：维护挂起的协程列表
3. **Task 来源**：`socket.read_async`、`process.sleep_async`、`task_ready`、`task_all`、`task_any`
4. **无标记**：处理器定义为 `fn(req) { ... }`，可以在体内 `await`
