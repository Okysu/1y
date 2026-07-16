"""
yin Web Framework Concurrency Benchmark
=======================================

Benchmarks the yin web framework (lib/yin.1y) at 10, 100, 1000, and 10000
concurrent requests. Tests both pure-1y concurrency (yin handler using
`await process.sleep_async`) and plain fast routes.

Usage:
    # Terminal 1: start the yin server
    cargo run --release -- run examples/yin_bench_server.1y

    # Terminal 2: run the benchmark
    python tests/bench_yin.py

The script:
  - Warms up with a single request
  - For each N in [10, 100, 1000, 10000]:
      * Sequential N requests  (baseline: N * single_latency)
      * Concurrent N requests   (the interesting number)
  - Reports throughput (req/s) and concurrency speedup ratio
  - Optionally tests a slow async handler to verify colorless async works
    (slow handler should NOT block fast handlers)
"""

import socket
import sys
import threading
import time
import urllib.request

# Bypass any system proxy (Windows often has one configured).
urllib.request.install_opener(
    urllib.request.build_opener(urllib.request.ProxyHandler({}))
)

BASE = "http://127.0.0.1:8080"
# Use a thread pool to cap concurrent threads at 10000 (Python's threading
# can handle this, but we avoid unbounded thread creation).
LEVELS = [10, 100, 1000, 10000]
TIMEOUT = 30  # per-request timeout in seconds


def fetch(path="/ping"):
    """Fetch BASE+path. Returns (status, elapsed_seconds, body_bytes)."""
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(BASE + path, timeout=TIMEOUT) as r:
            body = r.read()
            status = r.status
    except Exception as e:
        return -1, time.perf_counter() - t0, str(e).encode()
    return status, time.perf_counter() - t0, body


def test_sequential(n, path="/ping"):
    """Send n requests one after another. Returns total wall time."""
    t0 = time.perf_counter()
    ok = 0
    for _ in range(n):
        status, _, _ = fetch(path)
        if status == 200:
            ok += 1
    return time.perf_counter() - t0, ok


def test_concurrent(n, path="/ping"):
    """Send n requests concurrently via threads. Returns total wall time."""
    results = [None] * n

    def worker(i):
        results[i] = fetch(path)

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(n)]
    t0 = time.perf_counter()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=TIMEOUT + 5)
    total = time.perf_counter() - t0

    ok = sum(1 for r in results if r and r[0] == 200)
    return total, ok


def run_level(n, path="/ping"):
    """Run sequential + concurrent benchmark for a given request count."""
    print(f"\n{'─' * 64}")
    print(f"Level: {n} requests  (route: {path})")
    print(f"{'─' * 64}")

    # Sequential
    print(f"  [Sequential] {n} requests, one at a time...")
    seq_time, seq_ok = test_sequential(n, path)
    seq_rps = n / seq_time if seq_time > 0 else 0
    print(f"    Total: {seq_time*1000:.1f} ms  |  {seq_ok}/{n} ok  |  {seq_rps:.1f} req/s  |  {seq_time/n*1000:.2f} ms/req")

    # Concurrent
    print(f"  [Concurrent] {n} requests via threads...")
    conc_time, conc_ok = test_concurrent(n, path)
    conc_rps = n / conc_time if conc_time > 0 else 0
    print(f"    Total: {conc_time*1000:.1f} ms  |  {conc_ok}/{n} ok  |  {conc_rps:.1f} req/s  |  {conc_time/n*1000:.2f} ms/req")

    # Speedup ratio
    speedup = seq_time / conc_time if conc_time > 0 else 0
    print(f"  >> Speedup: {speedup:.2f}x  (concurrent vs sequential)")

    return {
        "n": n,
        "seq_time_ms": seq_time * 1000,
        "seq_ok": seq_ok,
        "seq_rps": seq_rps,
        "conc_time_ms": conc_time * 1000,
        "conc_ok": conc_ok,
        "conc_rps": conc_rps,
        "speedup": speedup,
    }


def test_slow_vs_fast():
    """Verify colorless async: a slow async handler should NOT block fast
    handlers. Sends 1 slow request + 5 fast requests concurrently."""
    print(f"\n{'─' * 64}")
    print("Colorless Async Test: 1 slow + 5 fast concurrent requests")
    print(f"{'─' * 64}")
    print("  Slow:  GET /slow  (handler awaits 500ms)")
    print("  Fast:  GET /ping  (~1ms)")

    results = []
    threads = []
    threads.append(threading.Thread(target=fetch_into, args=("/slow", "SLOW", results)))
    for i in range(5):
        threads.append(threading.Thread(target=fetch_into, args=("/ping", f"fast{i}", results)))

    t_global = time.perf_counter()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=TIMEOUT)
    t_end = time.perf_counter()

    results.sort(key=lambda x: x[1])
    print(f"\n  {'Label':<8} {'Start(ms)':>10} {'End(ms)':>10} {'Dur(ms)':>10} {'Status':<8}")
    print(f"  {'-' * 56}")
    for label, t0, t1, status, _ in results:
        print(f"  {label:<8} {(t0-t_global)*1000:>10.1f} {(t1-t_global)*1000:>10.1f} {(t1-t0)*1000:>10.1f} {status:<8}")

    slow_end = [r for r in results if r[0] == "SLOW"][0][2]
    fast_after_slow = sum(1 for r in results if r[0].startswith("fast") and r[2] > slow_end - 0.05)
    total_ms = (t_end - t_global) * 1000
    print(f"\n  Total wall time: {total_ms:.1f} ms")

    if fast_after_slow <= 1:
        print(f"  >> PASS: True concurrency — fast requests NOT blocked by slow handler.")
    elif fast_after_slow >= 4:
        print(f"  >> FAIL: Serial blocking — {fast_after_slow}/5 fast requests waited for slow.")
    else:
        print(f"  >> PARTIAL: {fast_after_slow}/5 fast requests waited for slow.")


def fetch_into(path, label, results):
    """Helper for test_slow_vs_fast: fetch and append results."""
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(BASE + path, timeout=TIMEOUT) as r:
            r.read()
            status = r.status
    except Exception as e:
        status = f"ERR"
    t1 = time.perf_counter()
    results.append((label, t0, t1, status, None))


def main():
    print("=" * 64)
    print("yin Web Framework — Concurrency Benchmark")
    print("=" * 64)
    print(f"  Server: {BASE}")
    print(f"  Levels: {LEVELS}")

    # Warmup + connectivity check
    print("\n[Warmup] Single request to /ping...")
    try:
        status, t, body = fetch("/ping")
        if status != 200:
            print(f"  FAILED: status {status}")
            sys.exit(1)
        print(f"  Status: {status}, Time: {t*1000:.2f} ms")
        print(f"  Body:   {body.decode()[:60]}")
    except Exception as e:
        print(f"  FAILED: {e}")
        print("\n  Is the yin server running? Start it with:")
        print("    cargo run --release -- run examples/yin_bench_server.1y")
        sys.exit(1)

    all_results = []
    for n in LEVELS:
        all_results.append(run_level(n, "/ping"))

    # Colorless async test
    test_slow_vs_fast()

    # Summary table
    print(f"\n{'=' * 64}")
    print("Summary")
    print(f"{'=' * 64}")
    print(f"  {'N':>6}  {'Seq(ms)':>10}  {'Conc(ms)':>10}  {'Seq(rps)':>10}  {'Conc(rps)':>10}  {'Speedup':>8}  {'Conc OK':>8}")
    print(f"  {'-' * 70}")
    for r in all_results:
        print(f"  {r['n']:>6}  {r['seq_time_ms']:>10.1f}  {r['conc_time_ms']:>10.1f}  {r['seq_rps']:>10.1f}  {r['conc_rps']:>10.1f}  {r['speedup']:>7.2f}x  {str(r['conc_ok'])+'/'+str(r['n']):>8}")

    print(f"\n{'=' * 64}")
    print("Done.")


if __name__ == "__main__":
    main()
