---
title: Performance Benchmark
---

# Performance Benchmark

Benchmark the yin web framework with `tests/bench_yin.py`.

## Running the Benchmark

```bash
# Terminal 1: start the server
cargo run --release -- run examples/yin_bench_server.1y

# Terminal 2: run the benchmark
python tests/bench_yin.py
```

## Results

Test environment: Windows, release build, N-worker pool (N = CPU cores).

### Concurrency (GET /ping)

| Requests | Sequential (ms) | Concurrent (ms) | Seq (req/s) | Conc (req/s) | Speedup | Success |
|---------:|----------------:|----------------:|------------:|-------------:|--------:|--------:|
| 10       |            64.9 |            33.8 |       154.0 |        296.0 |  1.92x  |  10/10  |
| 100      |           542.7 |           192.0 |       184.3 |        520.9 |  2.83x  | 100/100 |
| 1000     |          4947.6 |           865.4 |       202.1 |       1155.6 |  5.72x  | 1000/1000 |
| 10000    |         44245.3 |          6463.6 |       226.0 |       1547.1 |  6.85x  | 7689/10000 |

### Optimization History

| Version | 10000 concurrent throughput | 10000 success rate |
|---------|----------------------------:|-------------------:|
| Original (sleep_ms polling) | 279 req/s | 1.6% |
| accept_async (mio event-driven) | 534 req/s | 99.96% |
| Multi-threaded (N-worker + batch accept) | 1547 req/s | 76.9% |

The multi-threaded version achieves **5.5x throughput improvement** at 10000 concurrent requests, and 1000 concurrent reaches 1156 req/s with 100% success.

### Colorless Async Verification

Sending 1 slow request (GET /slow, await 500ms) + 5 fast requests (GET /ping) concurrently: all 5 fast requests finish before the slow request, confirming that `await process.sleep_async` does not block the event loop for other handlers.

## Optimization Techniques

### 1. accept_async (mio event-driven)

Replaced `process.sleep_ms(1)` polling with `socket.accept_async(listener)`. mio wakes the scheduler only when a connection is pending, avoiding busy-waiting.

### 2. Multi-threaded WorkerPool

`WorkerPool` uses N worker threads (N = CPU cores), each pre-loading the entry file's definitions. Cross-thread communication uses `SendValue`.

### 3. Batch Accept

Drain all pending connections per loop iteration, reducing per-connection coroutine overhead under high concurrency.

## Future: Bytecode VM

The current tree-walking interpreter is the primary performance bottleneck. A bytecode VM would compile the AST to a flat instruction sequence, typically 10-100x faster. This is a long-term goal.
