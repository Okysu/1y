---
title: Functions & Closures
---

# Functions & Closures

Functions are the fundamental building blocks of 1y programs. Like many functional languages, 1y treats functions as **first-class values**: a function can be assigned to a variable, passed as an argument, returned from another function, and stored in data structures. This "functions as data" design means 1y supports higher-order functions and closures natively, with no special syntax and no "function coloring."

## Named Functions

Use the `fn` keyword to declare a named function. The parameter list goes inside parentheses after the name, an optional return type is annotated with `-> Type`, and the body is wrapped in braces:

```1y
fn add(a, b) -> Int { a + b }

fn greet(name) -> Str { "Hello, {name}!" }

fn identity(x) { x }
```

The function body is **a single expression**, and its value is the function's return value — 1y does not require a `return` keyword to produce the function's main result; the value of the final expression is what gets returned.

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

Because `if` is an expression in 1y, it can serve directly as a function body. The `-> Type` return annotation is optional; when omitted, the return type is determined by the body's actual type.

## Anonymous Functions (Lambdas)

Besides named functions, you can create an anonymous function with `fn(params) { body }` and either bind it to a variable or use it inline:

```1y
let double = fn(x) { x * 2 };
let inc = fn(x) { x + 1 };

println(double(5));   // 10
println(inc(5));      // 6
```

Anonymous and named functions are indistinguishable at runtime — both are values of type `Func`. A named function is essentially equivalent to defining an anonymous function and then binding it to a name:

```1y
fn square(x) { x * x }
// is equivalent to
let square = fn(x) { x * x };
```

## Functions Are First-class

Because functions are values, you can pass one as an argument to another function or return a function from a function. Functions that accept functions as arguments (or return them) are called **higher-order functions**:

```1y
fn apply(f, x) { f(x) }

println(apply(double, 10));   // 20
println(apply(inc, 10));      // 11
```

Transforming collections is the most common stage for higher-order functions. For example, use `map` to map each element of a `Vec` to a new value:

```1y
let nums = [1, 2, 3, 4, 5];

let doubled = map(nums, fn(x) { x * 2 });
// [2, 4, 6, 8, 10]

let squared = map(nums, fn(x) { x * x });
// [1, 4, 9, 16, 25]
```

You can also return functions from functions, which lets you implement "partial application" or configurable behavior:

```1y
fn multiplier(n) {
    fn(x) { x * n }
}

let triple = multiplier(3);
let timesTen = multiplier(10);

println(triple(5));     // 15
println(timesTen(5));   // 50
```

## Closures: Capturing the Environment

An anonymous function **captures the lexical environment in which it is defined**. This means a closure can refer to variables from its enclosing scope, even after that scope has "exited." 1y closures capture their environment **by reference**: the closure sees the variable's current value rather than a snapshot taken at creation time (under immutability-first semantics this usually makes no difference, but it matters for reassignable variables).

```1y
fn make_adder(base) {
    fn(x) { x + base }   // captures the outer base
}

let addTen = make_adder(10);
println(addTen(5));      // 15
```

Closures make the combination of "configuration + behavior" natural. You can fix part of the parameters, obtain a specialized function, and then pass it along to a higher-order function:

```1y
fn below(threshold) {
    fn(x) { x < threshold }
}

let small = below(10);
let result = filter([1, 5, 15, 20, 3], small);
// [1, 5, 3]
```

Because capture is by reference, mutations to a captured variable made inside a closure are also visible outside (in scenarios where assignment is allowed):

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

## Function Types

Although 1y is dynamically typed, you can write function types in annotations to express "what a function should accept and return." A function type is written `fn(ArgTypes) -> RetType`:

```1y
// Conceptual type annotation:
// fn(Int) -> Int   denotes a function that takes an Int and returns an Int
```

`fn(Int) -> Int` reads as "a function from Int to Int." Such annotations make intent clearer when storing functions in structs or documenting an interface.

## Recursion

Functions can call themselves — this is **recursion**. Recursion is the natural way to handle recursively defined data (nested lists, trees, expression ASTs):

```1y
fn factorial(n) -> Int {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

fn fib(n) -> Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

println(factorial(10));   // 3628800
```

A more practical example is traversing a nested structure:

```1y
fn sum_tree(node) -> Int {
    match node {
        Leaf(v) => v,
        Node(left, right) => sum_tree(left) + sum_tree(right)
    }
}
```

::: warning Note: stack space
1y **does not optimize tail recursion**. Every recursive call consumes a real stack frame. For deep recursion (for example, processing data nested tens of thousands of levels deep), be aware that the stack can be exhausted — 1y provides a large stack to mitigate this, but for genuinely large-scale iteration it is better to rewrite the code as an explicit `loop` or `while` with an accumulator variable in place of recursion.
:::

Rewriting the factorial above as a loop eliminates any stack pressure:

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

## Summary

In 1y, functions are both a way to organize code and data that flows through your program. Named functions provide clear, reusable definitions; anonymous functions and closures let you construct behavior on demand and pass behavior as an argument. Mastering the combination of higher-order functions and closures lets you express powerful abstractions with very little boilerplate — and when you need to process recursive data, just remember: 1y gives you a generous stack, but for genuinely long iteration chains prefer `loop`.
