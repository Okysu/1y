---
title: Actor 模型
---

# Actor 模型

Actor 是 1y 并发的第一公民。一个 Actor 是一个拥有**隔离状态**的轻量执行单元，它通过**消息传递**与外界交互，一次只处理一条消息。这种"隔离 + 消息"的模型从根上消除了数据竞争：既然状态不共享，就不需要同步。

本页详细介绍 1y 中 Actor 的全部语法：如何用 `spawn` 创建、如何用 `!` 与 `?` 发送消息、如何用 `receive` 接收与 `reply` 回复、以及隔离状态与事件循环的运行机制，最后给出 `on` 处理器这一声明式写法。

## 创建 Actor：spawn

`spawn(initial_state) { body }` 创建并启动一个 Actor。`initial_state` 是该 Actor 的初始状态，在 `body` 内可以通过名为 `state` 的变量访问。`body` 通常是一个 `loop`，持续等待并处理消息。

```1y
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => state = state + n,
            Get => reply(state)
        }
    }
};
```

几点要点：

- `spawn` 立即返回一个 **Actor 句柄**，后续可用 `!` / `?` 与之通信。
- 初始状态 `0` 被**移动**进 Actor，从此只能由 Actor 自己读写，外部再也碰不到它。
- `body` 在 Actor 自己的执行上下文中运行，与调用方完全隔离。

## 发送消息：! 与 ?

1y 提供两种发送消息的运算符，分别对应两种通信语义。

### 发后即忘：!

`actor ! Message(args)` 把消息投递到 Actor 的信箱（mailbox），立即返回，**不等待**结果。

```1y
counter ! Inc(5);      # 告诉计数器加 5，不关心它何时处理完
```

`!` 适用于**命令型**消息：你只关心"把这件事交代下去"，不需要拿到结果。它是非阻塞的，发送方可以继续做别的事。典型的用法是驱动一个长期运行的服务——发一条命令，信任它最终会被处理。

### 请求/回复：?

`actor ? Message(args)` 发送消息并**阻塞等待回复**。它本质上是一次 `!` 加上一个隐式的回复通道；Actor 在处理这条消息时通过 `reply` 把结果送回，`?` 的返回值就是该回复。

```1y
let current = counter ? Get;   # 阻塞，直到 Actor 回复
print(current);
```

`?` 适用于**查询型**消息：你需要 Actor 的状态或计算结果。注意 `?` 会阻塞调用线程直到回复到达，因此在事件循环中应避免对长时间不回复的消息使用 `?`，以免饿死其他任务。一个良好的实践是：把 `?` 当作"同步函数调用"来用，只在确信 Actor 会迅速 `reply` 时发起。

## 接收消息：receive

`receive { Pattern => handler, ... }` 阻塞当前 Actor，直到收到一条与某个模式匹配的消息，然后执行对应的 `handler`。模式之间用逗号分隔。

```1y
receive {
    Inc(n) => state = state + n,
    Dec(n) => state = state - n,
    Get => reply(state),
    Reset => state = 0
}
```

`receive` 的语义：

- **阻塞**：在收到匹配消息前，Actor 挂起，不消耗 CPU。
- **模式匹配**：消息的构造器名（如 `Inc`、`Get`）与参数需匹配。1y 的模式匹配能力（字面量、解构、守卫等）在 `receive` 中同样适用。
- **一次一条**：每次 `receive` 只处理一条消息。把 `receive` 放在 `loop` 中即可持续服务。

通常 `receive` 写在 `loop` 内，形成一个长期运行的服务循环：

```1y
loop {
    receive {
        Inc(n) => state = state + n,
        Get => reply(state)
    }
}
```

## 回复：reply

`reply expr` 用于在处理 `?` 发来的消息时，把 `expr` 的值作为回复发回给调用方。

```1y
Get => reply(state)
```

要点：

- `reply` 只对 `?` 发起的请求有意义。对 `!` 发来的消息调用 `reply` 不会有效果——因为没有人在等待回复。
- 一个 handler 中至多 `reply` 一次。若 handler 不调用 `reply`，发起 `?` 的调用方将一直阻塞。
- `reply` 之后的代码仍会执行，但通常 `reply` 放在 handler 末尾以保持清晰。

## 隔离状态

每个 Actor 拥有完全隔离的状态，通过内置的 `state` 变量访问。这一隔离是 1y 并发安全的核心保证：

- **外部无法直接读写** Actor 的状态。唯一能影响状态的途径是发消息。
- **Actor 内部一次只处理一条消息**，因此对 `state` 的读写永远不存在并发访问。

```1y
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => state = state + n,
            Get => reply(state)
        }
    }
};
# 外部无法直接读 counter 的 state，只能 counter ? Get
```

正因如此，**你在 Actor 内部写代码时，就像在写单线程程序**——不需要锁、不需要原子操作、不需要考虑内存序。这是 Actor 模型最大的心智简化。需要共享状态时，你不需要为它加锁，只需要决定"谁来持有这份状态"——把它放进一个 Actor，让所有人通过消息与它交互。

## 事件循环

Actor 在 1y 中**单线程运行，由事件循环多路复用**。具体而言：

- 每个 Actor 不是操作系统线程，而是一个轻量的、可挂起/恢复的执行单元。
- 事件循环负责调度：当某个 Actor 的信箱有消息且该 Actor 处于可运行状态时，事件循环恢复它的执行，让它处理一条消息，处理完再挂起，转而调度下一个 Actor。
- 因为单线程，Actor 之间不存在抢占式并发，`state` 的访问天然串行。

这种模型意味着：大量 Actor 可以共存，而无需为每个 Actor 分配一个 OS 线程。一个 `?` 阻塞的是 Actor 自身的执行流，不会阻塞整个事件循环——只要其他 Actor 仍有消息可处理，事件循环就会继续推进。这也意味着 Actor 之间**不存在真正的并行**：如果你需要 CPU 密集的并行计算，应当把任务切片成消息分派给多个 Actor，让事件循环在它们之间交错推进，而不是指望它们同时跑在多个核上。

## on 处理器：receive 的替代

对于消息类型固定的服务型 Actor，逐条 `receive` 可能略显繁琐。1y 提供了 `on` 处理器作为替代：用 `on Name(params) { body }` 直接为每种消息声明一个处理函数。

```1y
let counter = spawn(0) {
    on Inc(n) {
        state = state + n
    }
    on Get {
        reply(state)
    }
};
```

`on` 与 `receive` 的关系：

- `on` 等价于一个自动展开的 `loop { receive { ... } }`，1y 会为每个 `on` 生成对应的 `receive` 分支并自动循环。
- 在 `on` handler 内同样用 `reply` 回复 `?` 请求、用 `state` 读写隔离状态。
- 选择 `receive` 还是 `on` 是风格问题：`receive` 更显式、适合需要复杂模式匹配或一次性接收的场景；`on` 更声明式、适合固定消息集合的服务。

## 完整示例：一个带容量的计数器

下面是一个稍完整的例子，展示 `spawn`、`!`、`?`、`receive` 与 `reply` 的协作：

```1y
import io;

# 一个有上限的计数器：超过上限时拒绝增加
let counter = spawn(0) {
    loop {
        receive {
            Inc(n) => {
                if state + n <= 100 {
                    state = state + n;
                    reply(true)
                } else {
                    reply(false)
                }
            },
            Get => reply(state)
        }
    }
};

counter ? Inc(30);       # 返回 true，state 现在是 30
counter ? Inc(80);       # 返回 false（30+80 超过 100），state 不变
let now = counter ? Get; # 返回 30
io.write("当前计数：" + now);
```

注意这里 `Inc` 用了 `?` 而非 `!`：因为我们想知道这次增加是否成功，需要拿到 `reply(true/false)`。若只是"加就完了"，用 `counter ! Inc(30)` 即可，Actor 内部也不必 `reply`。

## 小结

| 元素 | 语法 | 作用 |
|------|------|------|
| 创建 | `spawn(state) { body }` | 创建 Actor，返回句柄 |
| 发后即忘 | `actor ! Msg(args)` | 投递消息，不等待 |
| 请求/回复 | `actor ? Msg(args)` | 投递消息并阻塞等待回复 |
| 接收 | `receive { P => h, ... }` | 阻塞等待匹配消息 |
| 回复 | `reply expr` | 把结果回给 `?` 调用方 |
| 处理器 | `on Name(p) { body }` | 声明式消息处理 |
| 状态 | `state` | Actor 的隔离状态变量 |

Actor 模型用"隔离 + 消息"取代了"共享 + 锁"。把可变状态封进 Actor，用消息代替直接访问，并发就从"在哪里加锁"变成了"谁给谁发消息"——后者天然更清晰、更易组合。当确实需要跨多个状态进行原子协调时，再结合下一章的[事务内存 (STM)](./stm)。
