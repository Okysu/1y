---
title: 模式匹配
---

# 模式匹配

模式匹配是 1y 中最核心的控制结构之一。它让你能够**根据值的结构对值进行分支**,并在分支的同时把值拆解成更小的部分——这个过程称为**解构(destructuring)**。如果说 `if` 是基于布尔条件的分支,那么 `match` 就是基于形状的分支:它问的不是"这个值是真还是假",而是"这个值长什么样"。

模式匹配在 1y 中无处不在:处理枚举变体、解构结构体、检查集合的形状、捕获异常,都可以用 `match` 表达。与层层嵌套的 `if` 相比,`match` 让分支逻辑扁平、可读,并且鼓励你显式地处理每一种情况。

## 基本语法

一个 `match` 表达式由若干**分支(arm)**组成,每个分支的形式是 `Pattern => expr`,分支之间用逗号分隔。1y 会按从上到下的顺序依次尝试每个模式,执行第一个匹配成功的分支对应的表达式,其值即为整个 `match` 的值。

```1y
match value {
    Pattern => expr,
    Pattern => expr,
    _ => default
}
```

`match` 本身是一个表达式,因此它有值,可以出现在任何期待值的位置——例如直接绑定给一个变量:

```1y
let describe = match n {
    0 => "zero",
    1 => "one",
    _ => "many"
};
```

最后一个分支使用通配符 `_`,它会匹配任何值。当上面的所有具体模式都不命中时,`_` 兜底处理。如果你没有覆盖所有可能的情况且没有 `_` 兜底,运行时若没有模式匹配将抛出异常,因此养成"用 `_` 兜底"的习惯是稳妥的做法。

## 字面量模式

最简单的模式就是字面量。整数、字符串、布尔值和 `nil` 都可以直接作为模式:

```1y
match answer {
    42 => "the answer",
    0 => "nothing",
    _ => "something else"
}

match name {
    "Alice" => "hi Alice",
    "Bob" => "hi Bob",
    _ => "stranger"
}

match flag {
    true => "on",
    false => "off"
}
```

字面量模式做的是**相等比较**:值与字面量相等时即匹配。

## 变量绑定与通配符

模式中的小写标识符是一个**变量绑定**——它会匹配任何值,并把该值绑定到这个变量上,供分支右侧使用。下划线 `_` 是通配符,它同样匹配任何值,但不绑定。

```1y
match point {
    Point { x: px, y: py } => px + py   // 把字段绑定到 px、py
}

match opt {
    Some(x) => x * 2,                    // 把内部值绑定到 x
    None => 0
}

match anything {
    _ => "matched but ignored"           // 忽略具体值
}
```

变量绑定常常与字面量混合使用:字面量要求"相等",变量则"捕获其余"。

## Or 模式

当你希望多个模式走同一个分支时,可以用 `|` 把它们组合成 **or 模式**。只要其中任意一个子模式匹配,整个 or 模式就匹配:

```1y
match status {
    200 | 201 | 204 => "ok",
    400 | 404 => "client error",
    500 | 502 | 503 => "server error",
    _ => "unknown"
}

match c {
    "a" | "e" | "i" | "o" | "u" => "vowel",
    _ => "consonant"
}
```

这避免了为每个取值重复书写同一个分支表达式。

## Vec 模式

`Vec` 可以按位置解构。方括号里依次给出每个位置的模式;`..rest` 语法还能把剩余元素收集成一个新的 `Vec`:

```1y
match xs {
    [] => "empty",
    [x] => "one element: {x}",
    [a, b] => "two: {a} and {b}",
    [first, ..rest] => "first is {first}, rest has {Vec.len(rest)} items",
    _ => "something else"
}
```

`[first, ..rest]` 是一个非常实用的模式:它匹配**至少一个元素**的向量,把第一个元素绑定为 `first`,把剩下的元素绑定为 `rest`。注意,只有当向量真的满足模式所要求的最少元素数时才会匹配,否则继续尝试下一个分支。

## Map 模式

`Map` 模式按键来检查并解构。花括号里写出 `"key": subpattern`,只有当键存在且子模式也匹配时,该键才算匹配成功:

```1y
match config {
    {"host": h, "port": p} => "{h}:{p}",
    {"host": h} => "host only: {h}",
    _ => "no host"
}
```

Map 模式并不要求列出所有键——只要被列出的键都能匹配即可,其余键被忽略。这使得它非常适合"按需提取几个字段"的场景。

## Struct 模式

结构体通过 `TypeName { field: pattern, ... }` 的形式解构。冒号左侧是字段名,右侧是对该字段值的子模式:

```1y
type Point = { x: Int, y: Int };

let p = Point({ x: 3, y: 4 });

match p {
    Point { x: 0, y: 0 } => "origin",
    Point { x: px, y: py } => "point at ({px}, {py})"
}
```

这里 `Point { x: 0, y: 0 }` 用字面量精确匹配原点,而 `Point { x: px, y: py }` 把字段绑定到变量。和 Map 一样,Struct 模式只检查你列出的字段,未列出的字段不参与匹配。

## Variant 模式(枚举解构)

枚举变体是模式匹配最经典的用武之地。`Variant(args)` 模式匹配某个特定的变体,并把变体携带的值绑定出来:

```1y
enum Option { Some(Int), None }

match Some(42) {
    Some(x) => x,
    None => 0
}

enum Shape {
    Circle(Int),
    Rect(Int, Int),
    Point
}

match shape {
    Circle(r) => 3 * r * r,
    Rect(w, h) => w * h,
    Point => 0
}
```

`Point` 是无参数变体(Unit variant),它本身就是一个完整的模式。带参数的变体则需要给出对应数量的子模式。

## 守卫(Guard)

有时仅靠结构不足以表达判断条件。在模式后面加上 `if guard`,就能附加一个布尔表达式作为**守卫**:只有当模式匹配**且**守卫为真时,分支才被选中。

```1y
match n {
    x if x > 0 => "positive",
    x if x < 0 => "negative",
    _ => "zero"
}

match opt {
    Some(x) if x > 100 => "big",
    Some(x) => x,
    None => 0
}
```

守卫里可以引用模式中绑定的变量(如上例的 `x`),也可以引用外层作用域中的任何变量。若守卫为假,1y 不会报错,而是**继续尝试下一个分支**——这一点很重要,它让守卫成为"额外的筛选"而非"硬性断言"。

## 模式的组合与嵌套

所有模式都可以任意嵌套。你可以在 Vec 里放 Variant,在 Variant 里放 Struct,再用 or 模式和守卫层层包裹:

```1y
match request {
    [Some(cmd), ..rest] if cmd == "quit" => "bye",
    [Some(cmd), ..rest] => "run {cmd}",
    [None] => "empty request",
    _ => "malformed"
}

match event {
    Point { x: 0, y: 0 } | Point { x: px, y: py } if px == py => "diagonal or origin",
    Point { x: px, y: py } => "({px}, {py})"
}
```

嵌套模式让 `match` 能够精确描述复杂的数据形状,而无需事先把数据拆开再逐层 `if` 判断。

## 小结

模式匹配把"分支"与"解构"合二为一。通过字面量、变量、通配符、or、Vec、Map、Struct、Variant 这一套模式原语,再配合守卫,你几乎可以用一种声明式的方式描述"我期待什么样的数据,以及见到它之后该做什么"。当你发现自己写了长长的 `if` 链来检查值的类型和结构时,通常意味着该把它们重写成一个 `match` 了。
