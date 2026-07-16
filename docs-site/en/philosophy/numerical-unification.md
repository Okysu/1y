---
title: Numerical Unification
---

# Numerical Unification

In most languages, "number" is a fragmented concept. `int`, `long`, `float`, `double`, `BigInteger`, `BigDecimal`... programmers are forced to choose among half a dozen numeric types, and the price of choosing wrong is often catastrophic. 1y ends this fragmentation with a single decision: **only two numeric types, both arbitrary precision**.

## Two Types, One Arithmetic

1y provides two numeric types:

- **`Int`** — arbitrary-precision integer. No upper bound, no lower bound.
- **`Decimal`** — arbitrary-precision decimal. No precision loss.

Their relationship is governed by one simple rule: **the result of an operation is always the "wider" type**. When `Int` and `Decimal` meet in an operation, the `Int` is auto-promoted to `Decimal`. This rule frees programmers from the quagmire of type conversions.

```1y
let a = 10                  // Int
let b = 3.14                // Decimal
let c = a + b               // Decimal, 13.14
let d = a * b               // Decimal, 31.4
```

## The Promotion Rule for Division

Division is the operator most prone to bugs, because different languages handle "integer divided by integer" very differently. C, Java, and Go return the integer quotient (truncated); Python returns a float; JavaScript treats everything as a float. 1y adopts the most semantically honest rule: **if it divides evenly, return `Int`; otherwise promote to `Decimal`**.

```1y
let x = 10 / 2              // Int, 5
let y = 10 / 3              // Decimal, 3.3333...
let z = 7 / 2               // Decimal, 3.5
```

The key of this rule is that **the result always reflects mathematical truth**. `10 / 3` is not `3`, not `3.3333333333333335` (the tail of a floating-point approximation), but a genuine decimal. You will never lose precision because you "forgot to cast an operand to float."

## No Overflow, Really

In 1y, the following code is fully legal and correct:

```1y
let big = pow(10, 100)
// An integer: 1 followed by 100 zeros.
// In Java, this overflows a long into a strange negative number.
// In JavaScript, it becomes 1e+100, losing integer precision.
// In 1y, it is exactly what it should be.

let huge = factorial(500)
// 500 factorial is a 1135-digit integer.
// 1y holds it, computes with it, and prints it without fuss.
```

Compare this with real incidents caused by overflow in other languages: in 2014, *Candy Crush* zeroed out players' assets because a coin counter overflowed; in 2015, a Boeing 787 power-control unit crashed after 248 days due to an `int16` overflow; countless smart contracts have been drained by attackers exploiting `uint256` underflow. The root cause of these incidents is not programmer carelessness — it is **the language exposing the hardware's word size to business logic**. 1y refuses that exposure.

## Arbitrary Precision, Without Performance Worries

A common worry is: isn't arbitrary precision slow? The answer is — **for the vast majority of business code, it is entirely negligible**. 1y's `Int` uses a machine-native integer representation internally for small values, switching to a big-integer representation only when the value grows beyond the machine word. This means `1 + 1` has nearly the same cost as a native integer, while `factorial(500)` triggers the big-number path.

```1y
// Everyday arithmetic, fast path
let sum = 1 + 2 + 3                          // Int, 6

// Huge-number arithmetic, big-int path
let pow1000 = pow(2, 1000)                    // a 302-digit integer
```

`Decimal` works the same way: common decimals are represented efficiently, and only true arbitrary precision incurs extra cost. This "pay-as-you-go" design lets 1y be light enough for scripting yet precise enough for scientific computing.

## Why Decimal, Not Binary Floating-point

1y chooses `Decimal` (base-ten) over `binary float` (base-two) because **decimal is the language humans use to keep accounts**. Everything in finance, taxation, and accounting is decimal: `0.1` dollars is 10 cents, not some binary approximation that cannot be represented exactly.

```1y
// In JavaScript:
// 0.1 + 0.2 === 0.30000000000000004   ← classic trap

// In 1y:
let total = 0.1 + 0.2          // Decimal, 0.3   ← correct
let tax = 19.99 * 0.08         // Decimal, 1.5992
```

The `0.1 + 0.2 ≠ 0.3` quirk of binary floating-point is the source of countless financial bugs. 1y eliminates this problem at the language level: as long as your literals are decimal, the result of arithmetic is an exact decimal.

## One Mental Model, All Numbers

The ultimate payoff of numerical unification is the **disappearance of mental load**. You no longer need to:

- Decide between `int` and `long` — there is no `long`;
- Decide between `float` and `double` — there is no `float`/`double`;
- Worry about switching to `BigInteger` for big numbers — `Int` already is;
- Worry about switching to `BigDecimal` for money — `Decimal` already is;
- Worry about type conversions in mixed arithmetic — auto-promotion handles it.

What remains for you to care about is the problem itself. That is what 1y wants: the language handles the mechanical details, leaving your attention for the real logic.
