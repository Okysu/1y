---
title: 类型系统
---

# 类型系统

1y 是一门动态类型的语言,但每个值在运行时都有一个明确的类型。类型系统刻意保持精简:数值只有 `Int` 与 `Decimal` 两种,集合只有 `Vec`、`Map`、`Set` 三种,加上函数、自定义类型与并发原语,就构成了 1y 的全部值类型。本章逐一介绍它们,并解释持久化集合为何是 1y 不可变哲学的基石。

## 类型总览

下表列出 1y 的全部值类型及其字面量示例:

| 类型 | 字面量示例 | 说明 |
|------|------------|------|
| `Int` | `42`, `-1`, `1_000_000` | 任意精度整数(`num-bigint`) |
| `Decimal` | `3.14`, `0.5e10` | 任意精度十进制小数(`bigdecimal`) |
| `Str` | `"hello"`, `"""多行"""` | UTF-8 字符串,支持插值 |
| `Bool` | `true`, `false` | 布尔值 |
| `Nil` | `nil` | 空值/单元类型 |
| `Vec` | `[1, 2, 3]` | 持久化向量(`im::Vector`) |
| `Map` | `{"a": 1}` 或 `{x: 1}` | 持久化哈希映射,键为任意 `Value` |
| `Set` | `#{1, 2, 3}` | 持久化哈希集合 |
| `Func` | `fn(x) { x }` | 闭包,捕获定义环境 |
| `Native` | `println` | 内建或标准库函数 |
| `Variant` | `Some(42)` | 枚举变体实例:名字 + 参数 |
| `Struct` | `Point({x: 1, y: 2})` | 结构体实例:名字 + 字段映射 |
| `Actor` | `spawn ...` 的返回值 | Actor 句柄 |
| `Shared` | `shared 0` 的返回值 | 事务单元 |
| `Module` | `io`(`import io` 之后) | 模块导出的命名空间 |
| `Opaque` | `<tcp-stream>` | 原生资源句柄 |
| `LazyImport` | `lazy import` 绑定 | 延迟加载占位符 |

## 数值类型:Int 与 Decimal

1y 只有两种数值,且都是任意精度。这意味着**整数永远不会溢出**,小数也不会因二进制浮点而丢失精度——这是 1y 最重要的设计决策之一。

```1y
let big = 170141183460469231731687303715884105727;   // 128 位整数,毫无问题
let precise = 3.141592653589793238462643383279502884197;
let factorial_100 = factorial(100);   // 一个 158 位的整数
```

两种数值在算术运算中会自动提升:除法不能整除时提升为 `Decimal`,`Int + Decimal` 也提升为 `Decimal`。详见[表达式](./expressions)一章的提升规则。

## 基础值:Str、Bool、Nil

### Str

字符串是 UTF-8 编码的,用双引号或三引号包裹,支持插值:

```1y
let name = "alice";
let greeting = "hello, {name}!";        // 插值
let poem = """
第一行
第二行
""";                                     // 多行
```

字符串可以用 `+` 拼接,也可以用 `len`、`split`、`trim` 等标准函数操作。由于字符串内部使用引用计数(`Rc<String>`),复制开销很小。

### Bool 与 Nil

```1y
let ok = true;
let empty = nil;
```

`Bool` 只有 `true` 与 `false` 两个值,参与 `and`/`or`/`not` 逻辑运算。`Nil` 表示"没有值",常作为不返回有意义结果的函数的返回值(例如 `println`)。

## 持久化集合

1y 的三种集合——`Vec`、`Map`、`Set`——都是**持久化的**:对集合的"修改"不会改变原集合,而是返回一个共享了大部分结构的新集合。这是 1y 不可变哲学的核心。

### 持久化与结构共享

传统可变集合在添加元素时直接修改内存;持久化集合则不同——它返回一个新集合,但新集合与旧集合**共享未变部分的内部节点**。这意味着"复制"的成本接近 O(1),而非 O(n)。

```1y
let v = [1, 2, 3];
let v2 = push(v, 4);     // 返回新向量 [1, 2, 3, 4]
// v 仍然是 [1, 2, 3] —— 原值不变
println(count(v));        // 3
println(count(v2));        // 4
```

不可变性带来的好处是:**别名问题被根除**。你永远不必担心"另一个函数会不会偷偷改掉我的数据"。在并发场景下,数据可以在 Actor 之间自由传递,无需拷贝、无需同步。

### Vec — 持久化向量

```1y
let xs = [1, 2, 3];
let ys = push(xs, 4);         // [1, 2, 3, 4]
let first = xs[0];            // 索引访问,1
let combined = xs + ys;       // 拼接
```

`Vec` 内部基于 `im::Vector`,一个 RRB 树实现,提供接近 O(log₃₂ n) 的随机访问与 O(1) 的尾部追加。

### Map — 持久化映射

`Map` 的键可以是**任意 `Value`**,不限于字符串。字面量有两种写法:字符串键可省略引号,非字符串键需用 `key: value` 之外的形式。

```1y
let m = { x: 1, y: 2 };              // 字符串键可省略引号
let m2 = assoc(m, "z", 3);           // 添加键值对
let m3 = dissoc(m2, "x");             // 删除键
let v = get(m, "x");                  // 1
let also_v = m.x;                     // 字段访问是 get(m, "x") 的简写
```

注意 `{}` 会被解析为空块(`Nil`),而非空 `Map`。要构造空 `Map`,通常从一个占位条目开始再用 `dissoc` 移除,或使用 `assoc` 逐步构建。

### Set — 持久化集合

```1y
let s = #{1, 2, 3};
let s2 = insert(s, 4);               // 添加元素
let s3 = remove(s, 2);                // 删除元素
let has = contains(s, 1);             // true
```

`Set` 内部基于 `im::HashSet`,提供 O(1) 平均的成员判断。

## 函数:Func 与 Native

```1y
let double = fn(x) { x * 2 };         // Func 闭包
let result = double(5);                // 10
```

1y 的函数是一等值:可以作为参数传递、作为返回值、被存入集合。闭包以引用方式捕获定义环境,因此内部函数可以引用外层变量。`Native` 是内建函数(如 `println`)或标准库函数,从调用者的角度看,它与 `Func` 没有区别——两者都可以被调用、被管道传递。

## 自定义类型:Variant 与 Struct

1y 用 `enum` 与 `type` 两种声明来定义自定义类型,运行时分别产生 `Variant` 与 `Struct` 值。

### Variant(枚举变体)

`enum` 声明一组带名构造子,每个变体可携带零或多个参数:

```1y
enum Shape {
    Circle(Int),
    Rect(Int, Int)
}

let c = Circle(5);        // Variant 值:名字 "Circle" + 参数 [5]
let area = match c {
    Circle(r) => r * r,
    Rect(w, h) => w * h
};
```

`Variant` 在运行时由名字与参数列表组成,非常适合表达"和类型"(sum type)。

### Struct(结构体)

`type` 声明一个结构体类型,本质是命名字段的映射:

```1y
type Point = { x: Int, y: Int }

let p = Point({ x: 3, y: 4 });
let px = p.x;             // 字段访问,3
p.x = 42;                  // 字段赋值,返回新结构体
```

`Struct` 值由类型名与字段 `Map` 组成。字段访问 `p.x` 与字段赋值 `p.x = ...` 都直接操作这个映射。在模式匹配中,结构体可以用 `Point { x: px, y: py }` 的形式解构。

## 并发类型:Actor 与 Shared

```1y
let counter = spawn Counter();       // Actor 句柄
counter ! Inc(1);                     // 发后即忘
let n = counter ? Get;                // 同步请求/回复

let cell = shared 0;                  // 事务单元
transact {
    cell = cell + 1                   // 事务内读写
};
```

`Actor` 与 `Shared` 是 1y 并发模型的两种值类型,分别对应"消息传递"与"共享事务内存"。它们将在并发章节展开。

## 模块与不透明类型

`import` 之后,模块名绑定的是一个 `Module` 值,它的字段即模块导出:

```1y
import io;
println(str(io));        // <module io>
let content = io.read_to_string("file.txt");
```

`Opaque` 是原生资源句柄(如 TCP 连接、动态库句柄),由标准库函数创建,对 1y 代码而言是不透明的——你只能把它传回对应的原生函数,无法直接操作其内部。

## 接下来

类型定义了"有哪些值",而表达式定义了"如何计算出新值"。下一章[表达式与运算符](./expressions)将覆盖算术、比较、管道、字段访问与插值等全部运算形式。
