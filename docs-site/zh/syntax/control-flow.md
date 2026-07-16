---
title: 控制流
---

# 控制流

1y 是一门**表达式导向**的语言,这意味着控制流结构大多本身就有值。`if` 选择分支并返回值,`loop` 通过 `break` 把值送出循环,`try` 把可能失败的代码包裹起来并返回其结果或捕获的异常。理解每种结构的"值语义"是写出地道 1y 代码的关键——你很少需要靠副作用和中间变量来串联逻辑,而是让控制流直接产出结果。

## if 表达式

`if` 在 1y 中是表达式。它根据条件选择一个分支并返回该分支的值:

```1y
let sign = if x > 0 { 1 } else if x < 0 { -1 } else { 0 };
```

由于 `if` 有值,你可以直接把它用在赋值、函数返回、函数参数等位置,而无需引入临时变量:

```1y
fn grade(score) -> Str {
    if score >= 90 { "A" }
    else if score >= 80 { "B" }
    else if score >= 60 { "C" }
    else { "F" }
}

println(grade(95));   // A
println(grade(72));   // C
```

`else` 分支并非总是必需——但当你省略 `else` 时,未命中的分支会产出 `nil`。如果你打算使用 `if` 的值,最好确保两个分支都返回有意义的值,以避免意外的 `nil`。

## while 循环

`while` 在条件为真时反复执行循环体。它是一个**语句式**的循环,返回 `Nil`——它的用途是产生副作用(更新变量、累积结果),而非返回值:

```1y
let i = 1;
let sum = 0;
while i <= 10 {
    sum = sum + i;
    i = i + 1
};
println(sum);   // 55
```

`while` 适合"不确定要迭代多少次,但有明确终止条件"的场景,例如读取输入直到满足某个条件。

## loop 与 break

`loop` 创建一个无限循环,通常配合 `break` 退出。与 `while` 不同,`loop` 的 `break` 可以**携带一个值**——这个值就是整个 `loop` 表达式的返回值。这让 `loop` 成为一种通用的"循环并产出结果"的工具:

```1y
let n = 10;
let result = loop {
    if n == 1 { break 1 };
    n = n - 1
};
```

更实际的例子是用 `loop` 配合可变累加器实现迭代计算,并在结束时通过 `break value` 把结果送出:

```1y
fn factorial(n) -> Int {
    let acc = 1;
    let i = 1;
    loop {
        if i > n { break acc };
        acc = acc * i;
        i = i + 1
    }
}

println(factorial(5));   // 120
```

`break` 不带值时,等价于 `break nil`。`break` 也可以用于提前退出 `while`:

```1y
let i = 0;
while true {
    if i >= 5 { break };
    println(i);
    i = i + 1
}
```

## for 迭代

`for x in iter { ... }` 用于遍历 `Vec`(以及其他可迭代结构)。每一轮把当前元素绑定到变量 `x`,执行循环体:

```1y
let fruits = ["apple", "banana", "cherry"];

for f in fruits {
    println("I like {f}");
}
```

`for` 的循环体同样可以产生副作用或累积结果。和 `while` 一样,`for` 本身返回 `Nil`,若需要收集变换结果应使用高阶函数或显式累加:

```1y
let nums = [1, 2, 3, 4, 5];
let total = 0;
for x in nums {
    total = total + x
};
println(total);   // 15
```

## 异常:raise 与 try / rescue

1y 提供了基于异常的错误处理机制。`raise expr` 抛出一个异常——`expr` 可以是**任意值**(字符串、数字、结构体、枚举变体均可),不局限于某种专门的异常类型:

```1y
fn divide(a, b) {
    if b == 0 { raise "division by zero" };
    a / b
}
```

`try { ... } rescue [TypeName] as name { ... }` 捕获异常。`try` 块中的代码若抛出异常,1y 会将抛出值的类型与 `rescue` 后的类型名进行匹配(若不给出类型名则捕获所有异常);匹配成功时,把值绑定到 `as` 后的名字(若提供),并执行对应的处理块:

```1y
try {
    let r = divide(10, 0);
    println("result: {r}")
} rescue as msg {
    println("caught: {msg}")   // caught: division by zero
}
```

`rescue` 后面跟的是可选的**类型名**——它按名称匹配 `Variant` 或 `Struct`,因此你可以精确地分类处理不同异常:

```1y
enum AppError { Timeout, WithCode(Int) }

try {
    raise Timeout
} rescue Timeout {
    println("operation timed out, retrying")
} rescue WithCode as code {
    println("error code: {code}")
} rescue as other {
    println("unknown error: {other}")
}
```

这让你能够按类型来处理错误,而不是为异常单独发明一套机制。`try` 是一个表达式,它的值是 `try` 块成功时的值,或在 `rescue` 中处理块返回的值。

## ensure:清理块

`ensure { ... }` 定义一个**总是执行**的清理块。无论前面的代码是正常结束还是抛出了异常,`ensure` 中的代码都会运行。这让它成为释放资源(关闭文件、释放句柄)的理想位置:

```1y
try {
    let data = io.read_to_string("data.txt");
    process(data)
} rescue as e {
    println("error: {e}")
} ensure {
    // 无论是否出错,这里都会执行
    cleanup()
}
```

把 `try / rescue / ensure` 组合起来,你就得到了一套结构化的错误处理流程:尝试执行、按类型分类捕获、无论如何都做清理。

## 控制流即表达式

回顾这一章,你会发现 1y 的控制流有一个统一的特点:它们大多**有值**。`if` 选值,`loop` 经由 `break` 产出值,`try` 返回成功值或处理块的结果。唯一例外的是 `while` 与 `for`,它们返回 `Nil`,专为副作用循环而设。

这种设计意味着,你通常不需要先声明一个可变变量、再在各种分支里给它赋值,最后读取它。相反,你可以让控制流直接"算出"结果:

```1y
let label = if ok { "success" } else { "failure" };

let count = loop {
    // ... 某些条件成立时
    break final_count
};

let value = try {
    risky_parse(input)
} rescue as _ { 0 };
```

当你习惯了"让表达式自己携带值"的思路,1y 的代码会变得更紧凑、更少出错,也更接近数学式的描述。

## 小结

`if` 选择并返回值;`while` / `for` 是副作用循环,返回 `Nil`;`loop` 配合 `break value` 是带返回值的通用循环;`raise` 抛出任意值作为异常,`try / rescue` 按类型名捕获,`ensure` 保证清理。把这些结构当作"会产出值的表达式"来使用,你的 1y 程序会自然而然地流露出函数式的简洁与清晰。
