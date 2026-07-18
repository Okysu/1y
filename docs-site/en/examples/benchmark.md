---
title: Performance Benchmark
---

# Performance Benchmark

Benchmark the yin web framework with `tests/bench_yin.py`.

## Running the Benchmark

```bash
# Terminal 1: start the server (VM backend, default)
cargo run --release -- examples/yin_bench_server.1y

# Terminal 2: run the benchmark
python tests/bench_yin.py
```

To benchmark the legacy tree-walker for comparison, start the server with `run`:

```bash
cargo run --release -- run examples/yin_bench_server.1y
```

## Results

Test environment: Windows, release build, single-threaded event loop.

### VM (default backend) — Concurrency (GET /ping)

| Requests | Sequential (ms) | Concurrent (ms) | Seq (req/s) | Conc (req/s) | Speedup | Success |
|---------:|----------------:|----------------:|------------:|-------------:|--------:|--------:|
| 10       |            87.1 |            11.6 |       114.8 |        863.1 |  7.52x  |  10/10  |
| 100      |           292.7 |            65.4 |       341.7 |       1529.2 |  4.48x  | 100/100 |
| 1000     |          3941.8 |           608.1 |       253.7 |       1644.4 |  6.48x  | 1000/1000 |
| 10000    |         43168.7 |          5510.5 |       231.6 |       1814.7 |  7.83x  | 10000/10000 |

### Tree-walker (`1y run`) — Concurrency (GET /ping)

| Requests | Sequential (ms) | Concurrent (ms) | Seq (req/s) | Conc (req/s) | Speedup | Success |
|---------:|----------------:|----------------:|------------:|-------------:|--------:|--------:|
| 10       |            57.8 |            18.0 |       172.9 |        556.0 |  3.21x  |  10/10  |
| 100      |           332.3 |           99.0 |       301.0 |       1010.1 |  3.36x  | 100/100 |
| 1000     |          4518.5 |           595.1 |       221.3 |        808.0 |  7.59x  | 1000/1000 |
| 10000    |         55618.1 |          8279.9 |       179.8 |        671.8 |  6.72x  | 10000/10000 |

### VM vs Tree-walker (10000 concurrent)

| Backend | Throughput | Speedup vs TW |
|---------|-----------:|--------------:|
| Tree-walker | 672 req/s | 1.00x |
| **Bytecode VM** | **1815 req/s** | **2.70x** |

The VM delivers a **2.7x throughput improvement** at 10000 concurrent requests, and a **+85%** improvement in sequential throughput (232 vs 180 req/s). 10000/10000 requests succeed on both backends.

### Optimization History

| Version | 10000 concurrent throughput | 10000 success rate |
|---------|----------------------------:|-------------------:|
| Original (sleep_ms polling, tree-walker) | 279 req/s | 1.6% |
| accept_async (mio event-driven, tree-walker) | 534 req/s | 99.96% |
| Multi-threaded (N-worker + batch accept, tree-walker) | 1547 req/s | 76.9% |
| Single-threaded VM + scheduler tuning | 1815 req/s | 100% |

### Colorless Async Verification

Sending 1 slow request (GET /slow, `await process.sleep_async(500)`) + 5 fast requests (GET /ping) concurrently:

| Label | Duration (ms) | Status |
|-------|--------------:|-------:|
| SLOW  |         518.2 |    200 |
| fast0 |           4.0 |    200 |
| fast1 |           4.0 |    200 |
| fast2 |           3.8 |    200 |
| fast3 |           3.7 |    200 |
| fast4 |         517.2 |    200 |

4 of 5 fast requests finish in ~4 ms while the slow handler is still awaiting its 500 ms timer — confirming that `await process.sleep_async` does **not** block the event loop. The slow request itself completes normally in 518 ms. See [Colorless Async](../syntax/async) and the [Bytecode VM](../philosophy/bytecode-vm) page for how this works.

## Optimization Techniques

### 1. accept_async (mio event-driven I/O)

`socket.accept_async(listener)` registers the listener with `mio` (epoll/kqueue/IOCP). The scheduler only wakes when a connection is pending — no busy-waiting.

### 2. Timer min-heap

`process.sleep_async(ms)` now stamps a deadline hint on the Task. The scheduler parks timer-awaiting coroutines in a `BinaryHeap` keyed by deadline, so:

- `mio::poll` sleeps exactly until the next timer fires (or an I/O event arrives, whichever is sooner) — no 1 ms busy-polling.
- Each tick is O(log n) for timer push/pop, not O(n) scanning of all parked tasks.

### 3. Bounded `yield` ticks

Each `yield` advances the scheduler by at most 4 ticks before returning control to the accept loop. This lets slow handlers make progress (their timer fires within a `yield`) without trapping the accept loop for the full duration. Fast handlers (read → run → write, each yielding once) still complete within a single `yield` call.

### 4. Top-level `await` drives the scheduler

The HTTP accept loop is top-level code: its `await accept_async` runs outside any coroutine. The VM's top-level `await` busy-polls its own Task **but interleaves `drain_mailboxes_async` ticks between polls**, so parked coroutines (timers, I/O) keep advancing. Without this, a top-level await would starve all parked coroutines — e.g. a slow handler's 500 ms timer would never fire.

### 5. Upvalue closing on actor send

When `!` (ActorSend) queues a message containing a closure, the VM eagerly closes all open upvalues in that closure (and recursively in any nested values). This prevents the receiver's coroutine from reading dangling stack slots in the sender's (now-separate) `VmCtx`.

## Future Work

- VM support for `for`, `break`/`continue`, string interpolation, `try`/`transact` (see [Bytecode VM](../philosophy/bytecode-vm) — "What's Not Yet in the VM")
- Tail-call optimization for deeper recursion in the VM
- Multi-threaded VM (currently single-threaded; the tree-walker's `parallel` module is not yet VM-aware)
