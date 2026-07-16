---
title: Actor 模型
---

# Actor 模型

Actor 是 1y 并发的第一公民。一个 Actor 是一个拥有**隔离状态**的轻量执行单元，它通过**消息传递**与外界交互，一次只处理一条消息。这种"隔离 + 消息"的模型从根上消除了数据竞争：既然状态不共享，就不需要同步。

本页详细介绍 1y 中 Actor 的全部语法：如何用 `actor` 定义、如何用 `spawn` 启动、如何用 `!` 与 `?` 发送消息、如何用 `reply` 回复、以及隔离状态与事件循环的运行机制，最后给出 `on` 处理器这一声明式写法。

## 定义 Actor：actor + spawn

一个 Actor 通过 `actor Name { body }` 定义，body 中包含 `state` 声明、`on` 处理器与 `fn` 定义。通过 `spawn Name(args)` 创建并启动一个实例，它会返回一个 **Actor 句柄**，后续可用 `!` / `?` 与之通信。

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on get() { reply(count) }
}

let counter = spawn Counter();
```

几点要点：

- `spawn Name(args)` 立即返回一个 **Actor 句柄**，后续可用 `!` / `?` 与之通信。
- Actor 的状态由 body 中的 `state` 声明初始化。每个 `state name = value` 都创建一个隔离的、可变的绑定，只能由 Actor 自己读写，外部再也碰不到它。
- `body` 在 Actor 自己的执行上下文中运行，与调用方完全隔离。

## 发送消息：! 与 ?

1y 提供两种发送消息的运算符，分别对应两种通信语义。

### 发后即忘：!

`actor ! Msg(args)` 把消息投递到 Actor 的信箱（mailbox），立即返回，**不等待**结果。

```1y
counter ! inc(5);      // 告诉计数器加 5，不关心它何时处理完
```

`!` 适用于**命令型**消息：你只关心"把这件事交代下去"，不需要拿到结果。它是非阻塞的，发送方可以继续做别的事。典型的用法是驱动一个长期运行的服务——发一条命令，信任它最终会被处理。

### 请求/回复：?

`actor ? Msg(args)` 发送消息并**阻塞等待回复**。它本质上是一次 `!` 加上一个隐式的回复通道；Actor 在处理这条消息时通过 `reply` 把结果送回，`?` 的返回值就是该回复。

```1y
let current = counter ? get();   // 阻塞，直到 Actor 回复
print(current);
```

`?` 适用于**查询型**消息：你需要 Actor 的状态或计算结果。注意 `?` 会阻塞调用线程直到回复到达，因此在事件循环中应避免对长时间不回复的消息使用 `?`，以免饿死其他任务。一个良好的实践是：把 `?` 当作"同步函数调用"来用，只在确信 Actor 会迅速 `reply` 时发起。

## 处理消息：on

Actor 接收的每种消息都用一个 `on` 处理器声明：`on Name(params) { body }`。当消息发往 Actor 时，1y 根据消息的构造器名分派到同名的 `on` 处理器。处理器必须带括号（无参消息写作 `on Get() { ... }`）。

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on dec(n) { count = count - n }
    on get() { reply(count) }
    on reset() { count = 0 }
}
```

`on` 的语义：

- **按名分派**：消息的构造器名（如 `inc`、`get`）选中对应处理器，消息参数绑定到处理器的形参上。
- **一次一条**：Actor 一次只处理一条消息，因此对其 `state` 绑定的读写永远不存在并发访问。
- **body 内可模式匹配**：尽管分派是按名进行的，处理器 body 中仍可对参数或其他值使用 `match` 及 1y 的全部模式匹配能力（字面量、解构、守卫等）。

## 回复：reply

`reply expr` 用于在处理 `?` 发来的消息时，把 `expr` 的值作为回复发回给调用方。

```1y
on get() { reply(count) }
```

要点：

- `reply` 只对 `?` 发起的请求有意义。对 `!` 发来的消息调用 `reply` 不会有效果——因为没有人在等待回复。
- 一个 handler 中至多 `reply` 一次。若 handler 不调用 `reply`，发起 `?` 的调用方将一直阻塞。
- `reply` 之后的代码仍会执行，但通常 `reply` 放在 handler 末尾以保持清晰。

## 隔离状态

每个 Actor 拥有完全隔离的状态，通过 actor body 中的 `state` 绑定声明。这一隔离是 1y 并发安全的核心保证：

- **外部无法直接读写** Actor 的状态。唯一能影响状态的途径是发消息。
- **Actor 内部一次只处理一条消息**，因此对其 `state` 绑定的读写永远不存在并发访问。

```1y
actor Counter {
    state count = 0
    on inc(n) { count = count + n }
    on get() { reply(count) }
}

let counter = spawn Counter();
// 外部无法直接读 counter 的 count，只能 counter ? get()
```

正因如此，**你在 Actor 内部写代码时，就像在写单线程程序**——不需要锁、不需要原子操作、不需要考虑内存序。这是 Actor 模型最大的心智简化。需要共享状态时，你不需要为它加锁，只需要决定"谁来持有这份状态"——把它放进一个 Actor，让所有人通过消息与它交互。

## 事件循环

Actor 在 1y 中**单线程运行，由事件循环多路复用**。具体而言：

- 每个 Actor 不是操作系统线程，而是一个轻量的、可挂起/恢复的执行单元。
- 事件循环负责调度：当某个 Actor 的信箱有消息且该 Actor 处于可运行状态时，事件循环恢复它的执行，让它处理一条消息，处理完再挂起，转而调度下一个 Actor。
- 因为单线程，Actor 之间不存在抢占式并发，`state` 的访问天然串行。

这种模型意味着：大量 Actor 可以共存，而无需为每个 Actor 分配一个 OS 线程。一个 `?` 阻塞的是 Actor 自身的执行流，不会阻塞整个事件循环——只要其他 Actor 仍有消息可处理，事件循环就会继续推进。这也意味着 Actor 之间**不存在真正的并行**：如果你需要 CPU 密集的并行计算，应当把任务切片成消息分派给多个 Actor，让事件循环在它们之间交错推进，而不是指望它们同时跑在多个核上。

若要让 `!`（发后即忘）消息在程序退出前被处理，可使用 `yield;`——它会在当前位置清空待处理信箱。否则 `!` 消息会在程序结束时才被处理。

## 完整示例：一个带容量的计数器

下面是一个稍完整的例子，展示 `actor`、`spawn`、`!`、`?` 与 `reply` 的协作：

```1y
// 一个有上限的计数器：超过上限时拒绝增加
actor CappedCounter {
    state count = 0
    on inc(n) {
        if count + n <= 100 {
            count = count + n;
            reply(true)
        } else {
            reply(false)
        }
    }
    on get() { reply(count) }
}

let counter = spawn CappedCounter();
counter ? inc(30);       // 返回 true，count 现在是 30
counter ? inc(80);       // 返回 false（30+80 超过 100），count 不变
let now = counter ? get(); // 返回 30
println("当前计数：" + str(now));
```

注意这里 `inc` 用了 `?` 而非 `!`：因为我们想知道这次增加是否成功，需要拿到 `reply(true/false)`。若只是"加就完了"，用 `counter ! inc(30)` 即可，Actor 内部也不必 `reply`。

## 小结

| 元素 | 语法 | 作用 |
|------|------|------|
| 定义 | `actor Name { ... }` | 声明一个带状态与处理器的 Actor |
| 创建 | `spawn Name(args)` | 启动一个 Actor 实例，返回句柄 |
| 发后即忘 | `actor ! Msg(args)` | 投递消息，不等待 |
| 请求/回复 | `actor ? Msg(args)` | 投递消息并阻塞等待回复 |
| 回复 | `reply expr` | 把结果回给 `?` 调用方 |
| 处理器 | `on Name(p) { body }` | 声明式消息处理 |
| 状态 | `state name = value` | Actor 的隔离状态绑定 |

Actor 模型用"隔离 + 消息"取代了"共享 + 锁"。把可变状态封进 Actor，用消息代替直接访问，并发就从"在哪里加锁"变成了"谁给谁发消息"——后者天然更清晰、更易组合。当确实需要跨多个状态进行原子协调时，再结合下一章的[事务内存 (STM)](./stm)。
