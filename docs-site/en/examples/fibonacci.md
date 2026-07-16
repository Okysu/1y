---
title: Fibonacci
---

# Fibonacci

The Fibonacci sequence is the classic exercise when learning any language: `F(0) = 0`, `F(1) = 1`, and every subsequent term is the sum of the two before it. It makes a good teaching example because the same problem can be solved in radically different ways, and those implementations expose how a language actually feels in recursion, data structures, loops, and control flow. 1y has a killer feature here: `Int` is arbitrary precision, so `fib(100)` or even `fib(1000)` never overflows — you never switch types, never worry about a sign bit flipping.

## Version One: Naive Recursion

The implementation closest to the mathematical definition is direct recursion:

```1y
fn fib(n) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

println(fib(10))   // 55
```

This reads almost like the definition itself: when `n` is less than 2, return `n`; otherwise return the sum of the two preceding terms. `if` in 1y is an **expression** — its value is the value of the chosen branch — so the whole body can omit `return` and simply let the `if` expression be the function's return value.

The cost of naive recursion, however, is exponential. `fib(10)` needs about 177 calls, `fib(30)` needs over a million, and `fib(50)` will keep your program "stuck" for a long time. The problem is redundant computation: the result of `fib(n - 2)` has already been computed inside `fib(n - 1)`, yet nothing remembers it.

## Version Two: Memoization with a Shared Map

We obviously do not want the same subproblem solved over and over. A natural improvement is a "memo table" — store what you have computed and look it up next time. 1y's `shared` cell gives us a mutable reference that can be read and written from inside a closure, which is the perfect vehicle for a memo table:

```1y
fn fib_memo(n) -> Int {
    let memo = shared {0: 0, 1: 1};
    fn go(n) {
        match get(memo, n) {
            v if is_int(v) => v,
            _ => {
                let v = go(n - 1) + go(n - 2);
                memo = assoc(memo, n, v);
                v
            }
        }
    };
    go(n)
}

println(fib_memo(100))
```

Let us look closely at how this is built.

- `let memo = shared {0: 0, 1: 1}` creates a shared transactional cell wrapping a Map. The two base cases (`F(0) = 0`, `F(1) = 1`) are seeded up front with **integer keys** — note that keys and lookup values must use the same type, so we use `0` and `1`, not `"0"` and `"1"`.
- The inner function `go` closes over `memo`. Each call first does `get(memo, n)` — reading the shared cell returns the Map, and `get` looks up the key. When the key is absent, `get` returns `nil`, which falls through to the `_ =>` branch.
- `memo = assoc(memo, n, v)` writes the freshly computed `v` back into the shared cell. Because `memo` is a `shared` cell (not a plain `let` binding), this assignment persists across recursive calls — every subsequent `go` sees the updated table. This is what makes the memoization actually work.
- `assoc` returns a **new** Map (1y's Maps are persistent — structural sharing, no in-place mutation), and the shared cell is updated to point to this new version.

`get(memo, key)` looks up a key, `assoc(memo, key, val)` inserts or updates a key, and `dissoc(memo, key)` removes one — these three form the basic toolkit for working with a Map. Combined with `shared`, you get the convenience of mutable state without giving up the safety of persistent data structures.

## Version Three: Iteration

No matter how clever the recursion, plain iteration is the most intuitive and efficient approach. 1y provides `loop` and `while`, letting us compute Fibonacci with the simplest possible rolling-window method:

```1y
fn fib_iter(n) -> Int {
    let a = 0;
    let b = 1;
    let i = 0;
    while i < n {
        let next = a + b;
        a = b;
        b = next;
        i += 1
    };
    a
}

println(fib_iter(50))    // 12586269025
```

A few points deserve explanation:

- `let a = 0;` declares a variable that may be reassigned. In contrast to an immutable binding, 1y allows reassigning an existing name with `a = b` and supports compound assignment like `i += 1`. This gives loops the convenience they need without forcing a new name for every state update.
- `while i < n { ... }` is a loop statement that returns `Nil`. Inside, we use the classic "rolling window": each iteration updates `(a, b)` to `(b, a + b)`.
- The local `next` is redeclared on every iteration, which is perfectly fine — its scope is exactly one trip through the loop.

## Arbitrary Precision: Making fib(100) a Non-Issue

Now we reach where 1y truly shines. In C, Java, or Go, `fib(100)` overflows a 64-bit integer into a strange negative number; in JavaScript it becomes `3.542248481792619e+20`, losing integer precision. In 1y, you do nothing at all:

```1y
println(fib_iter(100))
// 354224848179261915075 — a 21-digit integer, exact
```

`Int` is represented internally with a native machine integer for small values and switches to a big-integer representation only when a value grows past the machine word. This means `fib(10)` and `fib(100)` run the very same code — you never switch types or libraries between the "small number" and "big number" cases.

## Trade-offs Across the Three Implementations

| Implementation | Time | Space | Best for |
|----------------|------|-------|----------|
| Naive recursion | O(φⁿ) | Call stack | Teaching, small n |
| Memoization (Map) | O(n) | O(n) Map | Reusing intermediate results |
| Iteration | O(n) | O(1) | Production, first choice |

Naive recursion wins on closeness to the definition and readability, but falls apart past `n > 30`; memoization brings the complexity down to linear at the cost of an extra Map; iteration is the plainest and fastest, using only two variables. **Unless you have a specific reason, prefer the iterative version in production code** — it is fast, memory-light, and handles a large value like `fib(1000)` effortlessly, which is exactly the class of headache that 1y's arbitrary-precision `Int` exists to take off your hands.
