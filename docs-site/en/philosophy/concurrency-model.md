---
title: Concurrency Model
---

# Concurrency Model

Concurrency is one of the hardest areas in software engineering, and its difficulty is often blamed on "concurrency itself." 1y takes a different stance: most of the difficulty of concurrency comes from **the wrong abstraction** — shared mutable memory plus locks. 1y replaces that with two proven abstractions: the **Actor model** for isolated state and message passing, and **Software Transactional Memory (STM)** for coordinated access to shared state. Both rest on immutable data, together forming a simple and safe concurrency model.

## Why Concurrency Needs Rethinking

First, consider the pain points of mainstream concurrency models:

- **Threads + locks**: data races, deadlocks, livelocks, and tricky lock granularity. Even the best programmers struggle to write correct lock-based code. Locks do not compose: two correctly-locked modules, combined, can deadlock.
- **async/await**: solves blocking but introduces "function coloring" — sync and async functions are two worlds that cannot freely interoperate. It also brings `Pin`, the `Future` ecosystem, and executor choice as extra complexity.
- **CSP (Go's channels)**: far better than locks, but still built on a shared runtime and goroutine scheduling, and a channel is itself a form of shared mutable state that must be handled carefully to avoid deadlock.

1y's judgment is: **isolation is simpler than synchronization**. Rather than having multiple flows of execution share one piece of memory and synchronize painstakingly, give each flow its own memory and let them communicate by passing messages. This is the core idea of the Actor model.

## The Actor Model: Isolation + Messages

The Actor is the first cornerstone of 1y concurrency. An Actor is a **lightweight process with private state**. It:

- Has fully isolated state that the outside cannot access directly;
- Owns a mailbox, receiving messages in order;
- Processes one message at a time, so its own state is inherently race-free.

```1y
actor Counter {
  state count: Int = 0

  on Incr { count = count + 1 }
  on Decr { count = count - 1 }
  on Get(reply) { reply ! count }
}
```

Creating and using an Actor:

```1y
let counter = spawn Counter
counter ! Incr                       # fire-and-forget
counter ! Incr
let n = counter ? Get                # request/reply, returns 2
```

Note the semantics of the two operators:

- **`!` (fire-and-forget)**: drops a message into the mailbox and returns immediately. The sender does not wait for a result. Suited to "command" messages — telling an Actor to do something without caring exactly when.
- **`?` (request/reply)**: sends a message and waits for a reply. Under the hood it is a `!` plus a promise, with the reply returned on an agreed channel. Suited to "query" messages — when you need an Actor's state or computation result.

Because each Actor processes only one message at a time, its internal state is never accessed concurrently. **When you write code inside an Actor, it is as if you were writing single-threaded code** — the single biggest mental simplification of the Actor model.

### Actors Compose Naturally

Actors can message each other, forming arbitrary topologies. An Actor can spawn child Actors, dispatch tasks, and collect results. Because state is isolated, this composition does not produce the deadlock problems that arise when combining locked modules:

```1y
# A pool manager that dispatches tasks to workers
actor Pool {
  state workers = List.map(0.until(8), _ => spawn Worker)

  on Submit(task) {
    let w = pick_idle(workers)
    w ! task
  }
}
```

## STM: Coordinated Access to Shared State

In some scenarios, pure message passing is awkward — for example, when multiple components need to read and write the same shared configuration, or when you need atomic updates across multiple data structures. For these cases, 1y provides **Software Transactional Memory (STM)**.

STM lets you mark a block of code as a "transaction"; reads and writes to shared references (`ref`) inside that block get **snapshot isolation**: the transaction sees a consistent snapshot taken at the start, and on commit, if it detects that a reference it read has been changed by another transaction, it retries automatically.

```1y
# Two accounts; transfer atomically
let alice = ref(100)
let bob   = ref(50)

atomically {
  alice := !alice - 30
  bob   := !bob   + 30
}
# Either both updates succeed, or both roll back — money never vanishes.
```

The core value of STM is that **atomicity composes**: you can combine two independent STM operations into a larger transaction without worrying about lock ordering between them. Compared with locks, this is a qualitative leap — locks do not compose; STM does.

```1y
# Compose multiple STM operations into one transaction
atomically {
  let balance = !alice
  if balance >= amount {
    alice := balance - amount
    ledger := List.push(!ledger, Transfer(alice, bob, amount))
    bob := !bob + amount
  }
}
```

## Division of Labor Between Actor and STM

Actor and STM are not an either/or choice; they are complementary:

| Scenario | Use | Reason |
|----------|-----|--------|
| Long-lived stateful service | Actor | State encapsulated inside the Actor; naturally modular |
| Request/response interaction | Actor (`?`) | One-to-one semantics are clear |
| Atomic updates across components | STM | Transactions compose well |
| Shared read-mostly config + occasional update | STM | Avoids a dedicated config Actor |
| Many independent workers | Actor | Isolation is stronger; failures are supervisable |

Rule of thumb: **default to Actor; reach for STM only when you need to atomically coordinate multiple shared references across boundaries**. This keeps the state boundaries of the system easy to follow.

## Why Snapshot Isolation Is Safe

STM's safety rests on two properties:

1. **Immutable data**: 1y values are immutable; a `ref` is merely "a replaceable label," and when replaced the old value still exists intact. A transaction reads a snapshot taken at a point in time, never a half-mutated value.
2. **Optimistic concurrency + retry**: a transaction takes no locks while running; on commit it checks whether the references it read still point to their original values. If any changed, the whole transaction retries. As long as conflicts are infrequent, this is more efficient than locking.

Because of immutability, retry is **idempotent** — re-running a transaction has no side effects (`!` messages inside a transaction are only actually sent after the commit succeeds). This is the subtlety of combining immutability with STM in 1y: immutability makes STM both correct and simple to implement.

## Comparison with Other Models

| Dimension | Threads+locks | async/await | CSP | 1y (Actor+STM) |
|-----------|---------------|-------------|-----|----------------|
| Data races | Common | Still possible | Fewer | Impossible (isolation) |
| Deadlocks | Common | Rare | Possible | Rare (no locks) |
| Function coloring | None | Severe | None | None |
| Composability | Poor | Medium | Medium | Good (STM) |
| Mental load | High | High | Medium | Low |

## Why Actor + STM Is Simpler and Safer

Returning to the opening judgment: **isolation is simpler than synchronization**. The Actor model eliminates "sharing" at the root — no sharing, no races. When sharing is genuinely needed, STM replaces "locks" (hard to reason about) with "transactions" (a concept programmers already know from databases).

The end result is that 1y concurrent code reads like sequential code: inside an Actor it is sequential; inside an STM transaction it is sequential. Concurrency shows up only in **topology** — who messages whom, who spawned whom — and designing at that level is inherently clearer than "which lock goes on which section." That is 1y's promise for concurrency: **make correctness the default, not a miracle**.
