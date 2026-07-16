---
title: 无色异步 —— 为什么没有 `async` 关键字
---

# 无色异步 —— 为什么没有 `async` 关键字

`async/await` 已经成为现代语言的标配：JavaScript、Python、Rust、C#、Kotlin 纷纷采纳。1y 走了一条不同的路：**没有 `async` 关键字，但有 `await`**。这并不矛盾——它是*无色异步*，与 Zig 采用的方案一致。本章解释原因。

## 真正的问题：函数染色

`async/await` 最深的代价不在语法，而在**函数染色**。一旦引入 `async`，函数就分裂成两个世界：

- **async 函数**只能被其他 async 函数调用，或在 async 运行时里调度；
- **普通函数**不能调用 async 函数（或只能阻塞等待）。

这种分裂是传染性的：一旦底层函数变成 `async`，调用链上的每个函数都得变成 `async`。社区称之为"async 传染"。它的实际后果包括：

- **API 碎片化**：同一个功能往往要同时提供 sync 和 async 两套接口（如 Rust 的 `Read` vs `AsyncRead`）。
- **类型复杂度**：Rust 的 `Future` 牵扯出 `Pin`、`Poll`、`Waker`、`Send` 约束——足以劝退新手。
- **运行时碎片化**：tokio、async-std、smol、embassy 互不兼容；选运行时等于选生态。
- **调试困难**：async 栈追踪是断的，常常指向运行时内部而非业务代码。
- **取消语义模糊**：future 被 drop 时，资源清理的时机不直观，容易泄漏。

这些复杂度不是"习惯就好"——它们是**结构性的**，根源在于函数有两种颜色这一基本事实。

## 1y 的答案：无色异步（Zig 风格）

1y 不通过给函数染色来解决并发问题。规则很简单：

> **没有 `async` 关键字。任何函数都能用 `await`。函数就是函数。**

这正是 Zig async 模型背后的洞见。编译器/运行时根据*正在 await 的是什么*来决定调用是否挂起，而不是根据函数定义上的标记。具体来说：

- 调用 `await socket.read_async(stream, n)` 的函数**可能**在那一挂起点挂起——但它仍然是普通 `fn`，可以从任何地方调用。
- 只调用同步代码的函数永远不会挂起——不需要标记。
- 同一个函数可以两者都做：同步工作，然后 `await` 一个 Task，然后更多同步工作。

```1y
// 普通 fn。没有 async 标记。但它能用 await。
fn handler(req) {
    let stream = get(req, "stream");
    // 暂无数据时挂起协程。
    // 等待期间其他连接继续被服务。
    let raw = await socket.read_async(stream, 65536);
    // 普通同步调用——无需标记。
    let parsed = parse_request(raw);
    // 再 await 一次——比如给 SSE 流调速。
    await process.sleep_async(500);
    build_response(parsed)
}
```

注意这里**没有**什么：没有 `async fn`，没有 `.await` 后缀，没有 `Pin<Box<dyn Future>>`，没有运行时选择。函数读起来和同步代码一模一样。唯一的新关键字是 `await`，而且它在*任何*函数里都能用。

## 工作原理：Task + 协程

底层上，1y 使用有栈协程（通过 `corosensei` 库）配合协作式调度器：

1. **`Task`** —— 代表异步操作的值。`socket.read_async(stream, n)` 返回 `Task<Str|Nil>`。`process.sleep_async(ms)` 返回 `Task<Nil>`。Task 有三种状态：`Pending`、`Ready(value)`、`Consumed`。

2. **`await task`** —— 挂起当前协程，向调度器注册该 Task。当 Task 变为 `Ready` 时，调度器用 Task 的值恢复协程。如果在协程外（顶层）调用，`await` 退化为同步忙轮询。

3. **`yield`** —— 并发心跳。在 Actor 事件循环里（如 `http.serve`），`yield` 为每条待处理消息派生一个协程并运行调度器。`await` 了尚未就绪 Task 的协程保持挂起；调度器每轮 tick 轮询它们的 Task，就绪后恢复执行。

4. **调度器** —— 单线程、协作式。`run_until_complete` 先跑所有就绪协程，再轮询挂起的 Task，重复直到全部完成或全部挂起（等 I/O）。并发不派生 OS 线程。

关键性质：**当协程 await 时，其他协程在跑**。慢 handler 不会阻塞其他进行中的连接。

## 为什么这比彩色 async/await 更好

| | 彩色 `async/await` | 1y 的无色 `await` |
|---|---|---|
| 函数标记 | 需要 `async fn` | 普通 `fn`——无标记 |
| 调用 async 函数 | 必须在 async 上下文里 | 任何函数都能 `await` |
| 传染性 | 有——async 沿调用链向上传染 | 无——函数只有一种颜色 |
| 运行时 | tokio/async-std/smol（互不兼容） | 内置调度器，无需选择 |
| 类型复杂度 | `Future` + `Pin` + `Poll` + `Waker` | `Task`——一个普通值 |
| 心智模型 | "这个是 async 吗？要 `.await` 吗？" | "返回 Task 就 `await` 它" |

## 与 Actor 的关系

无色异步不取代 Actor——而是互补。Actor 提供*结构化并发*（隔离、消息传递、监督）。`await` 提供*细粒度挂起*，在单个 Actor 的 handler 内部。

典型模式：每个连接派生一个 Actor，让 handler `await` 异步 I/O。Actor 模型给你隔离和基于消息的 API；`await` 让你在 handler 里做非阻塞 I/O，而不用把函数拆成 sync/async 两个世界。

```1y
actor Connection {
    on Handle(stream, handler) {
        // Actor handler 内部 await——无色异步。
        let raw = await socket.read_async(stream, 65536);
        let resp = handler(parse_request(raw));
        socket.write(stream, build_response(resp));
        socket.close(stream)
    }
}

// accept 循环：派生 Connection actor，yield 推进它们。
fn serve(addr, handler) {
    let listener = socket.listen(addr);
    socket.set_nonblocking(listener, true);
    loop {
        let stream = socket.accept(listener);
        if is_nil(stream) {
            try { yield } rescue { nil };
            process.sleep_ms(1)
        } else {
            let conn = spawn Connection();
            conn ! Handle(stream, handler)
        };
        try { yield } rescue { nil }
    }
}
```

## 权衡：我们放弃了什么

诚实地说，这个设计有代价。

- **协作式，非抢占式。** 永不 `await` 的协程会一直跑到完成。没有抢占。长 CPU 密集循环应定期 yield 或卸载到 `process.exec`。
- **单线程（目前）。** 真正的多核并行需要多进程（通过 `process`）或未来的多线程调度器工作。无色异步给的是并发，不是并行。
- **事件驱动 I/O。** 调度器用 `mio`（`epoll`/`kqueue`/IOCP）只在已注册流就绪时才等待，parked Task 只在 OS 报告就绪时才被轮询，而非每次 `yield` 都轮询全部。
- **Task 组合子保持精简。** `await` 是核心原语；`task_all([t1, ...])`、`task_any([t1, ...])`、`task_ready(value)` 用于组合 Task。长生命周期的并发状态请用 Actor。这是刻意的极简——与 Zig"一个原语做好"的哲学一致。

## 为什么这个权衡值得

1y 的目标用户是**写业务逻辑的程序员**，不是构建高性能网络框架的工程师。对前者，心智简单比峰值吞吐更重要；正确性比速度更重要；能信任并发原语比微调调度更重要。

彩色 `async/await` 把"如何高效使用线程"这个工程问题暴露给每个应用程序员，然后逼他们维护两个平行的函数宇宙。1y 把线程问题留给运行时，让程序员写**一种函数**——既能做同步工作，又能 `await` 异步 I/O——零仪式感。

这不是否认 async/await 的力量；而是拒绝**函数染色**。Zig 证明了可以有不带颜色分裂的异步挂起。1y 沿着这条路走：**有 `await` 无 `async`，并发不染色。**
