---
title: 函数与闭包
---

# 函数与闭包

函数是 1y 程序的基本组成单元。与许多函数式语言一样,1y 把函数视为**一等值(first-class value)**:函数可以被赋值给变量、作为参数传递、从函数中返回,也可以被存储在数据结构里。这种"函数即数据"的设计,让 1y 天然支持高阶函数与闭包,而无需任何特殊语法或"函数着色"。

## 命名函数

用 `fn` 关键字声明一个命名函数。参数列表写在函数名后的括号里,可选的返回类型用 `-> Type` 标注,函数体用花括号包裹:

```1y
fn add(a, b) -> Int { a + b }

fn greet(name) -> Str { "Hello, {name}!" }

fn identity(x) { x }
```

函数体是**一个表达式**,它的值就是函数的返回值——1y 没有 `return` 关键字来提前返回函数主结果(尽管存在 `return` 用于某些控制流场景,函数的返回值由最后一个表达式的值决定)。最后一句表达式的值即返回值。

```1y
fn max(a, b) -> Int {
    if a > b { a } else { b }
}

fn classify(score) -> Str {
    if score >= 90 { "A" }
    else if score >= 60 { "B" }
    else { "C" }
}
```

注意 `if` 在 1y 中是表达式,因此可以直接作为函数体返回。返回类型标注 `-> Type` 是可选的,省略时由函数体的实际类型决定。

## 匿名函数(Lambda)

除了命名函数,你还可以用 `fn(params) { body }` 创建匿名函数,并将其绑定到一个变量或直接内联使用:

```1y
let double = fn(x) { x * 2 };
let inc = fn(x) { x + 1 };

println(double(5));   // 10
println(inc(5));      // 6
```

匿名函数与命名函数在运行时没有区别——它们都是 `Func` 类型的值。命名函数本质上等价于"先定义匿名函数,再绑定到名字":

```1y
fn square(x) { x * x }
// 等价于
let square = fn(x) { x * x };
```

## 函数是一等值

正因为函数是值,你可以把它当作参数传给另一个函数,也可以从函数中返回一个函数。接受函数作为参数(或返回函数)的函数称为**高阶函数**:

```1y
fn apply(f, x) { f(x) }

println(apply(double, 10));   // 20
println(apply(inc, 10));      // 11
```

对集合进行变换是高阶函数最常见的舞台。例如,用 `map` 把一个 `Vec` 中的每个元素映射为新值:

```1y
let nums = [1, 2, 3, 4, 5];

let doubled = map(nums, fn(x) { x * 2 });
// [2, 4, 6, 8, 10]

let squared = map(nums, fn(x) { x * x });
// [1, 4, 9, 16, 25]
```

也可以让函数返回函数,借此实现"部分应用"或配置化的行为:

```1y
fn multiplier(n) {
    fn(x) { x * n }
}

let triple = multiplier(3);
let timesTen = multiplier(10);

println(triple(5));     // 15
println(timesTen(5));   // 50
```

## 闭包:捕获环境

匿名函数会**捕获它定义处所在的词法环境**。这意味着闭包能够引用其外层作用域中的变量,即使那个作用域已经"退出"。1y 的闭包**按引用捕获**环境:闭包看到的是变量的当前值,而不是创建闭包时的快照(在不可变优先的语义下,这通常没有区别,但对于可重新赋值的变量则很重要)。

```1y
fn make_adder(base) {
    fn(x) { x + base }   // 捕获了外层的 base
}

let addTen = make_adder(10);
println(addTen(5));      // 15
```

闭包让"配置 + 行为"的组合变得自然。你可以把一部分参数固定下来,得到一个专用的函数,再把它传递给高阶函数:

```1y
fn below(threshold) {
    fn(x) { x < threshold }
}

let small = below(10);
let result = filter([1, 5, 15, 20, 3], small);
// [1, 5, 3]
```

由于按引用捕获,闭包内部对捕获变量的修改也会反映到外部(在允许赋值的场景中):

```1y
let counter = 0;
let tick = fn() {
    counter = counter + 1;
    counter
};

println(tick());   // 1
println(tick());   // 2
println(tick());   // 3
```

## 函数类型

虽然 1y 是动态类型的,但你可以在标注中书写函数类型,用以表达"一个函数应当接受什么参数、返回什么"。函数类型写作 `fn(ArgTypes) -> RetType`:

```1y
// 概念上的类型标注:
// fn(Int) -> Int   表示接受一个 Int、返回 Int 的函数
```

`fn(Int) -> Int` 读作"一个从 Int 到 Int 的函数"。在把函数存入结构体或文档化接口时,这种标注能让意图更清晰。

## 递归

函数可以调用自身,这就是**递归**。递归是处理递归定义的数据(如嵌套列表、树、表达式 AST)的自然方式:

```1y
fn factorial(n) -> Int {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

fn fib(n) -> Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

println(factorial(10));   // 3628800
```

一个更实际的例子是遍历嵌套结构:

```1y
fn sum_tree(node) -> Int {
    match node {
        Leaf(v) => v,
        Node(left, right) => sum_tree(left) + sum_tree(right)
    }
}
```

::: warning 注意:栈空间
1y **不对尾递归做优化**。每一次递归调用都会真实地消耗栈帧。对于深度递归(例如处理上万层嵌套的数据),你需要意识到栈可能会被耗尽——1y 提供了较大的栈空间来缓解这一问题,但对于真正的大规模迭代,建议改写为 `loop` 或 `while` 形式的显式循环,用一个累加变量来替代递归。
:::

把上面的阶乘改写成循环形式,就避免了任何栈压力:

```1y
fn factorial_loop(n) -> Int {
    let acc = 1;
    let i = 1;
    while i <= n {
        acc = acc * i;
        i = i + 1
    };
    acc
}
```

## 小结

在 1y 中,函数既是组织代码的手段,也是可以在程序中流动的数据。命名函数用于清晰、可复用的定义;匿名函数与闭包用于按需构造行为、把行为当作参数传递。掌握"高阶函数 + 闭包"的组合,你就能用极少的样板代码表达出强大的抽象——而当你需要处理递归数据时,只需记住:1y 给了你充足的栈,但真正的长链条迭代请交给 `loop`。
