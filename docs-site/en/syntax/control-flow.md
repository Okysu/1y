---
title: Control Flow
---

# Control Flow

1y is an **expression-oriented** language, which means most control-flow structures carry a value of their own. `if` selects a branch and returns a value, `loop` sends a value out through `break`, and `try` wraps code that may fail and returns either its result or a caught exception. Understanding the "value semantics" of each construct is the key to writing idiomatic 1y — you rarely need to chain logic together with side effects and intermediate variables; instead, you let control flow produce results directly.

## The if Expression

`if` is an expression in 1y. It selects a branch based on a condition and returns that branch's value:

```1y
let sign = if x > 0 { 1 } else if x < 0 { -1 } else { 0 };
```

Because `if` has a value, you can use it directly in assignments, function returns, and function arguments without introducing a temporary variable:

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

The `else` branch is not always required — but when you omit it, the non-matching branch yields `nil`. If you intend to use the value of an `if`, it is best to ensure both branches return something meaningful, to avoid an unexpected `nil`.

## while Loops

`while` executes its body repeatedly as long as the condition is true. It is a **statement-like** loop that returns `Nil` — its purpose is to produce side effects (update variables, accumulate results), not to return a value:

```1y
let i = 1;
let sum = 0;
while i <= 10 {
    sum = sum + i;
    i = i + 1
};
println(sum);   // 55
```

`while` suits scenarios where "you don't know in advance how many times to iterate, but you have a clear termination condition," such as reading input until some predicate is satisfied.

## loop and break

`loop` creates an infinite loop, typically exited with `break`. Unlike `while`, `break` in a `loop` can **carry a value** — that value becomes the return value of the entire `loop` expression. This makes `loop` a general-purpose tool for "iterate and produce a result":

```1y
let n = 10;
let result = loop {
    if n == 1 { break 1 };
    n = n - 1
};
```

A more practical example uses `loop` with a mutable accumulator to implement an iterative computation, sending the result out with `break value` when done:

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

`break` with no value is equivalent to `break nil`. `break` can also be used to exit a `while` early:

```1y
let i = 0;
while true {
    if i >= 5 { break };
    println(i);
    i = i + 1
}
```

## for Iteration

`for x in iter { ... }` iterates over a `Vec` (and other iterable structures). Each iteration binds the current element to the variable `x` and executes the body:

```1y
let fruits = ["apple", "banana", "cherry"];

for f in fruits {
    println("I like {f}");
}
```

The body of a `for` can likewise produce side effects or accumulate results. Like `while`, `for` itself returns `Nil`; to collect transformed results, use a higher-order function or an explicit accumulator:

```1y
let nums = [1, 2, 3, 4, 5];
let total = 0;
for x in nums {
    total = total + x
};
println(total);   // 15
```

## Exceptions: raise and try / rescue

1y provides an exception-based error-handling mechanism. `raise expr` throws an exception — `expr` can be **any value** (a string, a number, a struct, an enum variant), not limited to some dedicated exception type:

```1y
fn divide(a, b) {
    if b == 0 { raise "division by zero" };
    a / b
}
```

`try { ... } rescue [TypeName] as name { ... }` catches exceptions. If code inside the `try` block raises, 1y tries to match the raised value's type against the type name after `rescue` (or catches everything when no type name is given); on a match, it binds the value to the name after `as` (if provided) and runs the corresponding handler block:

```1y
try {
    let r = divide(10, 0);
    println("result: {r}")
} rescue as msg {
    println("caught: {msg}")   // caught: division by zero
}
```

What follows `rescue` is an optional **type name** — it matches a `Variant` or `Struct` by name, so you can classify and handle different exceptions precisely:

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

This lets you handle errors by type without inventing a separate mechanism for exceptions. `try` is an expression: its value is the value of the `try` block on success, or the value returned by the `rescue` handler.

## ensure: Cleanup Blocks

`ensure { ... }` defines a cleanup block that **always runs**. Whether the preceding code completes normally or raises an exception, the code in `ensure` executes. This makes it the ideal place to release resources (close files, release handles):

```1y
try {
    let data = io.read_to_string("data.txt");
    process(data)
} rescue as e {
    println("error: {e}")
} ensure {
    // runs whether or not an error occurred
    cleanup()
}
```

Combining `try / rescue / ensure` gives you a structured error-handling flow: attempt execution, classify and catch by type, and clean up no matter what.

## Control Flow as Expressions

Looking back over this chapter, you will notice a unifying theme in 1y's control flow: most constructs **have a value**. `if` selects a value, `loop` produces a value through `break`, and `try` returns either a success value or a handler's result. The only exceptions are `while` and `for`, which return `Nil` and are reserved for side-effecting loops.

This design means you typically do not need to declare a mutable variable, assign to it across various branches, and then read it at the end. Instead, you let control flow "compute" the result directly:

```1y
let label = if ok { "success" } else { "failure" };

let count = loop {
    // ... when some condition holds
    break final_count
};

let value = try {
    risky_parse(input)
} rescue as _ { 0 };
```

Once you get used to the mindset of "let the expression carry its own value," your 1y code becomes more compact, less error-prone, and closer to a mathematical description.

## Summary

`if` selects and returns a value; `while` and `for` are side-effecting loops that return `Nil`; `loop` paired with `break value` is a general-purpose loop with a return value; `raise` throws any value as an exception, `try / rescue` catches by type name, and `ensure` guarantees cleanup. Treat these constructs as "expressions that produce values," and your 1y programs will naturally exude functional clarity and conciseness.
