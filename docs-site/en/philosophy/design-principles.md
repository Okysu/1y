---
title: Design Principles
---

# Design Principles

Every programming language is the concrete expression of a set of philosophical beliefs. 1y was not born to win a syntax contest; it was born from a plain question: **when we no longer need to compromise for the limitations of hardware, what should a functional language look like?**

Modern computers have enough memory and compute power that we no longer need to squeeze integers into 32 or 64 bits, nor force programmers to manage locks by hand in every concurrency scenario. Yet most mainstream languages still carry the legacy of last century's hardware constraints: fixed-width integers, mutable shared state, function coloring (the split between async and sync functions). 1y chooses to set down that legacy.

## Why 1y Exists

1y starts from two observations.

First, **numerical computation should never overflow**. Whether in cryptography, financial modeling, or combinatorics, programmers repeatedly hit the same class of bugs: `int32` overflow, `int64` overflow, floating-point precision loss. These are not programmer negligence — they are language design flaws that leak the hardware's word size to the programmer. 1y makes both `Int` and `Decimal` arbitrary precision from day one, eradicating overflow at the language level.

Second, **concurrency should not be this painful**. The threads-plus-locks combination is a breeding ground for data races and deadlocks; `async/await` solved blocking but introduced function coloring, `Pin`, and a complex future ecosystem. 1y took a different path: the Actor model handles message passing and isolated state, while Software Transactional Memory (STM) handles shared mutable state. Both are built on immutable data, so concurrent code reads as naturally as sequential code.

## Core Design Principles

### Immutability-first

In 1y, all values are immutable by default. Once a value is created, it never changes. Data structures are **persistent**: "adding" an element to a list does not modify the original list — it returns a new list that shares most of its structure.

```1y
let xs = [1, 2, 3]
let ys = List.push(xs, 4)
# xs is still [1, 2, 3]; ys is [1, 2, 3, 4]
```

This sounds inefficient, but persistent data structures achieve near-O(1) copy cost through **structural sharing**. More importantly, immutability eliminates aliasing problems at the root: you never worry that "another function might silently mutate my data." In concurrent settings, this means data can flow freely between Actors with no copying and no synchronization.

### Expression-oriented

1y is an expression language: almost everything has a value. `if` is an expression, `match` is an expression, and blocks are expressions. There is no split between "statements" and "expressions," and therefore no trap of "this function early-returned `undefined`."

```1y
let status =
  if score >= 90 then "A"
  else if score >= 60 then "B"
  else "C"
```

Expression-orientation brings programs closer to mathematics: a program is "a computation," not "a sequence of instructions." It also makes refactoring safer — any piece of code can be extracted into a function because it already has a value.

### Numerical Unification

1y has only two numeric types: `Int` (arbitrary-precision integer) and `Decimal` (arbitrary-precision decimal). Arithmetic auto-promotes: `Int + Decimal` yields `Decimal`; division that does not divide evenly auto-promotes to `Decimal`. The programmer never chooses between `int`, `long`, `float`, `double`, `BigInteger`, and `BigDecimal` — the language chooses correctly on your behalf.

```1y
let a = factorial(500)        # a 1135-digit integer, no problem
let b = 10 :pow 100           # 1 followed by 100 zeros
let c = 1 / 3                 # 0.3333... (Decimal), not 0
```

See [Numerical Unification](./numerical-unification) for the full story.

### Concurrency as First-class

Concurrency is not an add-on library; it is the core of the language. Both Actor and STM are built in with dedicated syntax:

```1y
let counter = spawn Counter.init(0)
counter ! Incr                       # fire-and-forget
let current = counter ? Get          # request/reply
```

Concurrency primitives are not "advanced features" — they are everyday tools for every 1y programmer. See [Concurrency Model](./concurrency-model).

## How These Principles Shape the Language

The four principles reinforce one another. Immutability lets Actors pass references safely without copying; expression-orientation makes an STM transaction body naturally "a computed value" with clear rollback semantics; numerical unification removes boundary bugs from type conversion, making concurrent numerical computation more reliable.

The combined result: **1y programs read like a specification**. Code describes "what is," not "how to do it." When you read a piece of 1y code, you rarely need to simulate a stepwise state machine in your head — because there is no mutation, only the flow of values.

## Comparison with Other Languages

| Dimension | 1y | Java/Go | Rust | Haskell |
|-----------|-----|---------|------|---------|
| Integers | Arbitrary precision | Fixed-width (overflow-prone) | Fixed-width | Arbitrary precision |
| Mutability | Immutable by default | Mutable by default | Immutable by default | Immutable by default |
| Concurrency | Actor + STM | Threads/locks, channels | Threads, async | STM, async |
| Function coloring | None | None | Yes (async) | Yes |
| Mental load | Low | Medium (locks) | High (borrowing) | Medium |

1y is closest to Haskell in values, but deliberately avoids Haskell's type-system complexity and the surprise costs of lazy evaluation. 1y is strict, with a lightweight type system, so that "the correctness of functional programming" can be enjoyed by more programmers.

## Small but Complete

1y's syntax surface is tiny: `let`, `fn`, `match`, `if`, `actor`, `atomically`, the module system, and FFI — it fits on one sheet of paper. But "small" does not mean "weak":

- Arbitrary-precision numbers, sufficient for finance and scientific computing;
- Persistent data structures, sufficient for complex data transformations;
- Pattern matching, sufficient to express intricate branching logic;
- Actor + STM, sufficient to build highly concurrent systems;
- FFI, sufficient to reuse the full power of the Rust ecosystem.

This "small but complete" philosophy is 1y's practice of the belief that a language should be restrained. A language's power lies not in how many keywords it offers, but in how much expressive space it carves out with the fewest primitives. 1y chooses to be a sparrow with all its organs intact: every design choice exists for a real problem, and no feature exists to show off.

As you read through this documentation, you will notice that 1y has no macros, no type classes, no effect system, no async. Each of these "absences" is deliberate — each corresponds to a kind of complexity that 1y believes can be reached with simpler primitives. The following chapters unpack the reasoning behind each of these choices.
