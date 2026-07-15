---
title: Hello World
---

# Hello World

Every programmer's journey begins with the same line. In 1y, asking the computer to say hello takes a single line of code — yet behind that line lies the language's plainest and most central promise: **everything is an expression, values are immutable by default, and numbers never overflow**. This example starts from the simplest possible output and grows into a small program that reads user input and builds a greeting with string interpolation, walking you through the basic skeleton of a 1y program.

## The Minimal Version

We begin with one line:

```1y
println("Hello, World!")
```

Save it as `hello.1y` and run it from the shell:

```
1y run hello.1y
```

You will see `Hello, World!` printed to the terminal.

What happened in that line? `println` is a global built-in function, available without any `import`. It accepts any value, converts it to a displayable string, appends a newline, and writes the result to standard output. `"Hello, World!"` is a string literal wrapped in double quotes. That is the smallest runnable unit of a 1y program — a single expression statement.

## Wrapping the Greeting in a Function

A hard-coded greeting is not very flexible. In a real program, we usually want the greeting to change based on the caller's name. Functions in 1y are defined with the `fn` keyword:

```1y
fn greet(name) -> Str {
    "Hello, {name}!"
}

println(greet("World"))
println(greet("1y"))
```

Let us unpack this line by line:

- `fn greet(name) -> Str` declares a function named `greet` that takes one parameter, `name`, and is annotated to return `Str`. 1y is dynamically typed, so the type annotation serves mainly as documentation — a way to communicate intent to future readers (including yourself).
- The body is a single line: `"Hello, {name}!"`. Notice the **string interpolation** — `{name}` is replaced at runtime with the current value of the variable `name`. There is no `+` concatenation and no `format` template; interpolation is baked directly into the string literal.
- Because 1y is expression-oriented, the value of the last expression in the body is the function's return value — no explicit `return` is needed. `"Hello, {name}!"` is both the last line of the body and the return value of `greet`.

Running this produces:

```
Hello, World!
Hello, 1y!
```

::: tip How do I write a literal brace?
Because `{` and `}` serve as interpolation markers inside strings, a literal brace must be escaped with a backslash: `"\{ not a variable \}"`.
:::

## Adding User Input

Real programs interact with people. Let us ask the user for their name and greet them back. Reading standard input requires the `io` module:

```1y
import io;

fn greet(name) -> Str {
    "Hello, {name}!"
}

print("What is your name? ");
let name = io.read_line();

match name {
    s if is_str(s) => println(greet(trim(s))),
    nil => println("Hello, silent friend!")
}
```

This program introduces several new concepts, which we explain one at a time.

**`import io;`** brings the standard library's `io` module into the current scope, bound to the name `io`. 1y's module system is straightforward: an imported module becomes a namespace, and you access its functions with dot notation like `io.read_line`. `import` is eager by default; if you would rather defer loading until first use, write `lazy import io;`, which is handy for keeping startup fast.

**`print`** differs from `println` only in that it does not append a newline. We use it for the prompt `What is your name? ` so the cursor stops right after the question mark to await input, which reads more naturally.

**`io.read_line()`** reads one line of text from standard input. When input ends (for example, when the user presses Ctrl+D / Ctrl+Z), it returns `nil`. This is an operation that "may fail," and 1y uses `nil` to denote an absent value rather than throwing — because reaching EOF is a normal end-of-input path in an interactive program, not an exceptional one.

**`let name = io.read_line();`** binds the result to the immutable variable `name`. Note the trailing semicolon — it marks this as a "statement" whose value is discarded, whereas the last expression in a block without a semicolon becomes the block's value. This is the subtle way 1y distinguishes "statements" from "value expressions."

**`match name { ... }`** is the idiomatic way to handle a value that may be absent. We branch on `name` with pattern matching:

- `s if is_str(s)` matches any string, and the guard `is_str(s)` ensures it really is a string (not `nil`). The binding `s` refers to the matched value. `is_str` is a built-in type predicate; 1y offers a family of these, including `is_int`, `is_map`, and more.
- `nil` matches the end-of-input case and provides a fallback greeting.
- Note that we call `trim(s)` on `s` — the string returned by `read_line` usually ends with a newline `\n`, and we strip it to get a clean name. `trim` is a built-in string function that removes leading and trailing whitespace.

At runtime, the program pauses and waits for your input:

```
What is your name? Ada
Hello, Ada!
```

## Recap

This example is short, yet it covers several cornerstones of a 1y program: output with `println`, defining functions with `fn`, string interpolation with `{name}`, bringing modules in with `import`, binding values with `let`, and branching with `match`. More importantly, it shows 1y's expressive style — no boilerplate, no unnecessary ceremony, the program reads like a direct statement of intent. In the examples that follow, we will build progressively more complex systems the same way, from numerical computation to concurrent programming.
