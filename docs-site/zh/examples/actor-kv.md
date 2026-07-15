---
title: Actor 键值存储
---

# Actor 键值存储

键值存储是并发编程里最经典的练手项目之一:它有状态、有读写竞争、需要清晰的 API 边界。本例用 1y 的 Actor 模型从零搭一个内存键值存储,你会看到"隔离 + 消息传递"如何让一个本来需要锁的程序读起来像顺序代码。我们将从单个 Actor 开始,扩展到支持 `Get`/`Set`/`Delete` 的完整接口,再讨论"发后即忘"与"请求/回复"两种交互风格的取舍,最后用多个 Actor 实现命名空间隔离。

## Actor 模型回顾

在 1y 中,Actor 是一个拥有私有状态的轻量进程。它通过信箱(mailbox)按顺序接收消息,一次只处理一条,因此**自身状态永远不存在并发访问**。定义一个 Actor 用 `actor` 关键字,创建实例用 `spawn`:

```1y
actor Counter {
    state count = 0
    on inc() { count = count + 1; reply(count) }
}

let c = spawn Counter();
let n = c ? inc();      // 请求/回复,返回 1
```

- `state count = 0` 声明 Actor 的私有状态,初始值为 `0`。这块状态对外完全不可见,只能通过消息间接读写。
- `on inc() { ... }` 定义一个消息处理器:收到 `inc` 消息时执行花括号内的代码。
- `reply(count)` 把一个值作为回复返回给发送方。只有使用 `?`(请求/回复)发送的消息才会等待这个回复。
- `spawn Counter()` 创建 `Counter` 的一个实例,返回它的句柄。
- `c ? inc()` 发送 `inc` 消息并**阻塞等待**回复,返回值就是 `reply` 的内容。

## 第一个 KV Actor

我们先把存储的核心逻辑放进一个 Actor。状态是一张 `Map`,消息有 `put`(写入)、`get`(读取)、`has`(判断键是否存在)、`delete`(删除)和 `size`(返回条数):

```1y
actor KVStore {
    // 注意:`{}` 会被解析成空块而非空 Map。
    // 用这个技巧得到一个真正的空 Map 作为初始状态。
    state data = dissoc({ __init: 0 }, "__init")

    on put(key, val) { data = assoc(data, key, val); reply(nil) }
    on get(key) { reply(get(data, key)) }
    on has(key) { reply(get(data, key) != nil) }
    on delete(key) { data = dissoc(data, key); reply(nil) }
    on size() { reply(count(data)) }
}

let store = spawn KVStore();
store ? put("name", "alice");
store ? put("city", "paris");
println(store ? get("name"));          // alice
println(str(store ? has("city")));     // true
store ? delete("city");
println(str(store ? has("city")));     // false
println(str(store ? size()));          // 1
```

这里有几处关键细节,值得逐一说清。

**初始状态的空 Map。** 1y 的 `{}` 会被解析成一个空代码块(值为 `Nil`),而不是空 Map。这是一个历史遗留的语法权衡。为了让 `data` 真正是一张 Map,我们用了一个惯用技巧:先构造 `{"__init": 0}`,再用 `dissoc` 把占位键删掉,得到一张货真价实的空 Map。在你自己的程序里,只要初始状态非空,直接写 `{"k": v}` 即可。

**用 `assoc` 更新状态。** 1y 的 `Map` 是持久化的:`assoc(data, key, val)` 不会就地修改 `data`,而是返回一张**共享了大部分结构**的新 Map。我们用 `data = assoc(...)` 把 Actor 的状态"指向"那张新 Map。因为只有当前 Actor 在处理消息,这个赋值是安全的——没有其他执行流会看到中间状态。

**`get` 的双重含义。** 注意 `get` 既是消息名,也是内置函数名。在 `on get(key) { reply(get(data, key)) }` 里,前者是消息标签,后者是查表函数,二者不会冲突。`get(data, key)` 在键不存在时返回 `nil`,这正是 `has` 用 `!= nil` 判断的依据。

## 发后即忘 vs 请求/回复

Actor 之间有两种发送方式,选哪种取决于你是否关心结果:

```1y
// 发后即忘:不等待回复
store ! put("name", "bob");

// 请求/回复:阻塞等待 reply 的值
let v = store ? get("name");
```

- **`!`(发后即忘)** 把消息投入信箱后立即返回,发送方继续往下走。它适合"命令"型消息——告诉 Actor 去做某件事,但不必立刻确认结果。在上面的例子里,`put` 其实可以用 `!` 发送,因为我们只关心写入是否发生,不关心它的回复(`nil`)。
- **`?`(请求/回复)** 发送消息并阻塞,直到 Actor 在处理器里调用 `reply`。它适合"查询"型消息——你需要 Actor 的状态或计算结果。

一个实践原则:**查询用 `?`,写入看场景**。如果写入后立刻需要读回确认,用 `?`;如果只是批量灌数据,用 `!` 让发送方不必阻塞。需要注意的是,`!` 投递的消息会在程序结束的"排空阶段"被集中处理,所以紧跟其后的 `?` 可能还看不到 `!` 的效果——这是 Actor 信箱的时序特性,而非 bug。

## 命名空间:多个 Actor 隔离状态

Actor 模型的一个天然优势是:同一个 Actor 定义可以 spawn 出多个互不干扰的实例。我们用它实现命名空间隔离——每个命名空间是一个独立的 `KVStore` 实例:

```1y
let users = spawn KVStore();
let sessions = spawn KVStore();

// 两个存储互不影响
users ? put("alice", "admin");
sessions ? put("token-42", "alice");

println(users ? get("alice"));          // admin
println(sessions ? get("token-42"));    // alice
println(str(users ? size()));           // 1
println(str(sessions ? size()));        // 1
```

`users` 和 `sessions` 拥有各自独立的 `data` Map。哪怕两个 Actor 同时收到消息,它们的状态也不会相互污染,因为 Actor 的状态本来就是隔离的——这正是"隔离比同步简单"的体现。要扩展成支持任意命名空间,只需再做一层"管理 Actor",它维护一张 `Map<namespace, KVStore句柄>`,收到请求后转发给对应的子 Actor。

## 何时该用 Actor 做存储

Actor 模式非常适合**长生命周期、有明确边界的有状态服务**。它的代价是:跨 Actor 的原子操作不直观(比如"原子地把 key 从 A 迁到 B"需要额外的协议),且查询响应是阻塞的。如果你需要的是跨多个数据结构的一次性原子更新,[事务性计数器](./transactional-counter)里介绍的 STM 往往更顺手。两者并非二选一,而是分工互补——经验法则是:**默认用 Actor,只有当多个共享引用必须一起原子变更时才转向 STM**。
