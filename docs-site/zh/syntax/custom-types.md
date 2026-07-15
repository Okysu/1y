---
title: 自定义类型
---

# 自定义类型

1y 内置了丰富的数据类型——`Int`、`Decimal`、`Str`、`Bool`、`Vec`、`Map`、`Set` 等等——但真实世界的程序往往需要描述自己领域内的数据形状。1y 提供了两种自定义类型的手段:**结构体(struct)**用于把若干字段打包成一个命名产品,**枚举(enum)**用于表达"这个值是若干互斥情形之一"的标签联合(tagged union)。两者都与模式匹配紧密配合,构成了 1y 数据建模的基础。

## 结构体:用 type 声明

`type` 关键字创建一个**结构体类型**。它的语法是 `type Name = { field: Type, field: Type, ... }`,字段之间用逗号分隔:

```1y
type Point = { x: Int, y: Int };
type User = { name: Str, age: Int };
type Rect = { width: Int, height: Int };
```

声明类型只是定义了一个"形状"。要创建该类型的值,需要调用以类型名为名字的**构造函数**,传入一个 Map 字面量来填充字段:

```1y
let p = Point({ x: 3, y: 4 });
let u = User({ name: "Alice", age: 30 });
let r = Rect({ width: 10, height: 5 });
```

构造函数的写法是 `Name({ field: value, ... })`——类型名后跟一个用花括号包裹的字段 Map。这种设计让结构体的构造在视觉上与解构对称,也便于和 Map 模式互通。

## 字段访问

结构体的字段通过 `.` 运算符访问:

```1y
let p = Point({ x: 3, y: 4 });
println(p.x);   // 3
println(p.y);   // 4

let u = User({ name: "Alice", age: 30 });
println("{u.name} is {u.age} years old");   // Alice is 30 years old
```

你也可以在表达式中读取字段、参与运算,从而基于现有结构体派生新值:

```1y
fn area(r) -> Int { r.width * r.height }

let r = Rect({ width: 10, height: 5 });
println(area(r));   // 50
```

由于 1y 的值默认不可变,你不能"修改"某个字段,而是构造一个新的结构体来表达更新后的状态。这在并发场景下尤其重要——不会有人因为读取旧值而拿到被改了一半的数据。

## 结构体与模式匹配

结构体最强大的用法是与 `match` 配合解构。`Point { x: px, y: py }` 模式会把字段绑定到给定变量,而 `Point { x: 0, y: 0 }` 这样的字面量模式则精确匹配特定取值:

```1y
type Point = { x: Int, y: Int };

fn describe(p) -> Str {
    match p {
        Point { x: 0, y: 0 } => "origin",
        Point { x: px, y: py } => "({px}, {py})"
    }
}

println(describe(Point({ x: 0, y: 0 })));   // origin
println(describe(Point({ x: 3, y: 4 })));   // (3, 4)
```

更多关于模式匹配的细节,参见[模式匹配](./pattern-matching)一章。

## 枚举:用 enum 声明

当某个值只能取"几种互斥情形之一"时,用 `enum` 声明一个**标签联合**。每个情形称为一个**变体(variant)**:

```1y
enum Option { Some(Int), None }

enum Color {
    Red,
    Green,
    Blue
}

enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}
```

变体可以携带不同数量和类型的参数:

- **无参数(Unit)变体**:如 `Red`、`None`、`Point`,本身就是完整的值。
- **单参数变体**:如 `Some(Int)`、`Circle(Int)`,携带一个值。
- **多参数变体**:如 `Rect(Int, Int)`,携带多个值。

构造一个枚举值,就是调用对应的变体名:

```1y
let a = Some(42);
let b = None;
let c = Circle(5);
let d = Rect(3, 4);
let e = Red;
```

## 用 match 处理枚举

枚举与 `match` 天生一对。`match` 能够根据变体的标签分支,并把变体携带的值绑定出来:

```1y
enum Option { Some(Int), None }

fn unwrap_or(opt, default) -> Int {
    match opt {
        Some(x) => x,
        None => default
    }
}

println(unwrap_or(Some(42), 0));   // 42
println(unwrap_or(None, 0));       // 0
```

对于多参数变体,模式里按位置给出对应数量的绑定变量:

```1y
enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}

fn area(s) -> Int {
    match s {
        Circle(r) => 3 * r * r,
        Rect(w, h) => w * h,
        Point => 0
    }
}

println(area(Circle(2)));     // 12
println(area(Rect(3, 4)));    // 12
println(area(Point));         // 0
```

枚举强制你**穷举**所有变体——配合 `_` 兜底或逐一列出,你能在编译/运行时就发现"忘了处理某种情况"的问题。这是 `enum + match` 比一堆布尔标志和 `if` 链更安全的关键原因。

## 小结

`type` 定义结构体——把若干命名字段打包成一个产品,通过 `Name({...})` 构造,通过 `.` 访问字段,通过 `Name { ... }` 模式解构。`enum` 定义标签联合——把若干互斥情形列在一起,通过 `Variant(args)` 构造,通过 `match` 分支处理。两者一起,让你能以接近问题域的方式建模数据,再借助模式匹配把"数据的形状"和"对每种形状的处理"清晰地对应起来。
