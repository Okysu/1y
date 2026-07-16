---
title: 多线程
---

# 多线程 (Multi-threading)

1y 通过内置的 `parallel` 模块提供用户级多线程。基于 `WorkerPool`（N 个 worker 线程，每个 CPU 核一个），每个 worker 预加载入口文件的定义后保持存活，接受函数调用。

## API

| 函数 | 签名 | 说明 |
|------|------|------|
| `parallel.cores` | `() -> Int` | CPU 核数 |
| `parallel.call` | `(Str, Vec) -> Value` | 同步调用命名函数 |
| `parallel.spawn` | `(Str, Vec) -> Handle` | 异步调用，返回句柄 |
| `parallel.join` | `(Handle) -> Value` | 等待异步任务完成 |
| `parallel.map` | `(Str, Vec<Vec>) -> Vec` | 并行映射 |

## 同步调用

```1y
fn heavy_compute(n) {
    let s = 0;
    let i = 0;
    while i < n { s = s + i; i = i + 1 };
    s
}

// 阻塞直到 worker 完成，返回结果
let r = parallel.call("heavy_compute", [1000000]);
```

## 异步调用

```1y
// 立即返回句柄
let h1 = parallel.spawn("heavy_compute", [1000000]);
let h2 = parallel.spawn("heavy_compute", [2000000]);

// 阻塞等待每个结果
let r1 = parallel.join(h1);
let r2 = parallel.join(h2);
```

## 并行映射

```1y
// 四个调用在不同 worker 上并发执行
let results = parallel.map("heavy_compute", [[1000], [2000], [3000], [4000]]);
```

## 约束

- 函数通过**名称**（字符串）调用，不是闭包引用
- 参数和返回值必须是 `SendValue` 兼容类型：Int、Str、Bool、Nil、Vec、Map、Set、Variant、Struct
- 函数、shared cell、actor、task、opaque resource 不能跨线程传递
- Worker 线程只加载定义（FuncDef、ActorDef、TypeDef、EnumDef、Import），不执行副作用语句

## 工作原理

1. **WorkerPool**：N 个 worker 线程，每个拥有独立的 `Interpreter`（Rc-based，!Send）
2. **预加载**：worker 启动时加载入口文件的定义（函数、actor、类型、import）
3. **任务分发**：通过共享 `mpsc` 频道，任何 worker 可以领取下一个任务
4. **跨线程通信**：使用 `SendValue`（Value 的 Send+Sync 子集）传递参数和返回值

## 与 Actor 的关系

`parallel` 模块用于 **CPU 密集型并行**（多核计算），Actor 用于 **并发 I/O**（连接管理、消息传递）。两者互补：

- `parallel.map` 适合并行计算（如批量数据处理）
- Actor 适合并发 I/O（如 HTTP 服务器每连接一个 actor）
