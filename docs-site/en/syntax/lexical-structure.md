---
title: Lexical Structure
---

# Lexical Structure

Every programming language begins with characters. Before 1y can understand your code, the lexer splits the source character stream into tokens — comments, identifiers, literals, operators, and punctuation. This chapter covers each of 1y's lexical rules, helping you understand which character sequences are valid and what they parse into.

## Comments

1y provides two kinds of comments. They are discarded during lexing and do not affect program semantics.

### Line Comments

A line comment begins with `//` and continues to the end of the line. Ideal for brief inline notes:

```1y
// This is a line comment
let x = 1;   // a trailing comment is also valid
```

### Block Comments

A block comment begins with `/*` and ends with `*/`, and may span multiple lines. **Block comments are nestable** — unlike in C/C++/Java, you never have to worry about an inner comment terminating the outer one prematurely:

```1y
/* This is a
   multi-line comment */
let y = 2;

/* Outer comment
   /* Inner comment, legal nesting */
   Outer continues */
let z = 3;
```

Nestable comments are especially handy when you want to temporarily comment out a large block of code that itself contains block comments.

## Identifiers

Identifiers name variables, functions, types, and modules. The rules are straightforward:

- They start with a **letter** or an underscore `_`;
- Subsequent characters are letters, digits, or underscores.

```1y
let name = "alice";
let _count = 0;
let total2 = 42;
let __internal = nil;
```

Identifiers are case-sensitive: `Name` and `name` are two distinct identifiers. Note that a character sequence starting with a digit is parsed as a numeric literal, so identifiers cannot begin with a digit.

## Numeric Literals

1y has only two numeric types — `Int` (arbitrary-precision integer) and `Decimal` (arbitrary-precision decimal). The lexer distinguishes them by the presence of a decimal point or an exponent.

### Integers (Int)

An integer literal is a sequence of digits, optionally with underscores inserted as visual separators (which do not affect the value):

```1y
let a = 42;
let b = 1_000_000;          // equal to 1000000
let huge = 170141183460469231731687303715884105727;   // arbitrary precision, no overflow
```

Underscores may appear between any digits, making large numbers easier to read: `1_000_000` is exactly equal to `1000000`.

### Decimals (Decimal)

A numeric literal containing a decimal point or an exponent marker is parsed as a `Decimal`:

```1y
let pi = 3.14;
let precise = 3.141592653589793238462643383279502884197;
let exponent = 0.5e10;        // exponent notation
let big_exp = 1.5e10;
```

`Decimal` is also arbitrary precision, suitable for financial and scientific computing. The arithmetic promotion rules (such as `Int + Decimal` yielding `Decimal`) are covered in the [Expressions](./expressions) chapter.

## String Literals

1y strings are UTF-8 encoded and support interpolation and multi-line writing.

### Double-Quoted Strings

The most common form, enclosed in double quotes:

```1y
let s = "hello, world";
let empty = "";
```

### Triple-Quoted Strings

Enclosed in three double quotes `"""`, these can span multiple lines — ideal for large blocks of text:

```1y
let poem = """
Roses are red,
Violets are blue.
""";
```

Triple-quoted strings preserve internal newlines and indentation, making them useful for templates, documentation, or JSON fragments.

### String Interpolation

Within a double-quoted or triple-quoted string, `{expr}` is replaced by the value of the expression — this is the most common way to construct strings in 1y:

```1y
let name = "world";
let n = 42;
println("hello, {name}! answer = {n}");

// Any expression is allowed inside the braces
let xs = [1, 2, 3];
println("sum = {fold(xs, 0, fn(a, b) { a + b })}");
```

### Escaping and Literal Braces

Strings support common escape sequences: `\n` (newline), `\t` (tab), `\\` (backslash), `\"` (double quote). **When you need a literal brace character in a string**, escape it as `\{` and `\}`, otherwise it will be interpreted as interpolation:

```1y
let json_text = "\{\"name\": \"alice\"\}";   // literal {"name": "alice"}
let path = "C:\\Users\\me";                  // literal backslashes
```

This is essential when writing text containing many braces, such as JSON or templated strings.

## Keywords

The following words are reserved in 1y and cannot be used as identifiers:

| Category | Keywords |
|----------|----------|
| Bindings & declarations | `let` `fn` `type` `enum` `struct` |
| Control flow | `if` `else` `match` `while` `for` `in` `loop` `break` `continue` `return` |
| Exceptions | `raise` `try` `rescue` `ensure` |
| Concurrency | `actor` `on` `spawn` `reply` `receive` `shared` `transact` `retry` |
| Modules | `import` `lazy` `as` |
| Logic | `and` `or` `not` |
| Literals | `true` `false` `nil` |

If you genuinely need to refer to a keyword-named entity (for example, calling an external function named `type`), you can typically do so via module field access like `m.type` — this is discussed in the [Statements](./statements) and [Modules](./modules) chapters.

## Operators and Punctuation

1y uses the following operators and punctuation:

| Category | Symbols |
|----------|---------|
| Arithmetic | `+` `-` `*` `/` `%` |
| Comparison | `<` `<=` `>` `>=` `==` `!=` |
| Assignment | `=` `+=` `-=` `*=` `/=` `%=` |
| Pipe | `\|>` |
| Logic | `and` `or` `not` (words, not symbols) |
| Index/field | `.` `[` `]` |
| Messaging | `!` `?` |
| Dereference | `*` (used to read/write `shared` cells) |
| Delimiters | `(` `)` `{` `}` `,` `;` `:` `->` `=>` `\|` `..` |

Here `!` sends a fire-and-forget message to an Actor, and `?` performs a synchronous request/reply; `*` reads and writes transactional cells in a `shared` context. These are expanded upon in the concurrency chapters.

## What's Next

Lexical structure is the lowest layer of the language. Now that you understand how tokens are formed, the next step is to see what types 1y offers and what their literals look like — see [Type System](./types).
