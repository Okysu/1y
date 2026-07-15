"""
1y HTTP server concurrency benchmark.

Tests:
  1. Single request latency (baseline)
  2. Sequential N requests (expected: N * baseline)
  3. Concurrent N requests (the interesting one)

If concurrent ≈ sequential, the server processes requests in series
(no true request-level concurrency — expected for 1y's sync handler model).
If concurrent << sequential, the server handles requests in parallel.
"""

import threading
import time
import urllib.request

# Bypass any system proxy (Windows often has one configured).
proxy_handler = urllib.request.ProxyHandler({})
opener = urllib.request.build_opener(proxy_handler)
urllib.request.install_opener(opener)

URL = "http://127.0.0.1:8080/hello"
N = 10  # number of requests per test


def fetch(url=URL):
    """Fetch a single URL, return (status_code, elapsed_seconds)."""
    t0 = time.perf_counter()
    with urllib.request.urlopen(url, timeout=10) as r:
        body = r.read()
        status = r.status
    return status, time.perf_counter() - t0, body


def test_sequential():
    """Send N requests one after another."""
    print(f"\n[Sequential] {N} requests, one at a time...")
    t0 = time.perf_counter()
    results = []
    for _ in range(N):
        results.append(fetch())
    total = time.perf_counter() - t0
    print(f"  Total: {total*1000:.1f} ms  ({total/N*1000:.1f} ms/req)")
    return total


def test_concurrent():
    """Send N requests simultaneously via threads."""
    print(f"\n[Concurrent] {N} requests, all at once via threads...")
    results = [None] * N

    def worker(i):
        results[i] = fetch()

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(N)]
    t0 = time.perf_counter()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=15)
    total = time.perf_counter() - t0

    # Check all succeeded
    ok = sum(1 for r in results if r and r[0] == 200)
    print(f"  Total: {total*1000:.1f} ms  ({ok}/{N} succeeded)")
    if results and results[0]:
        print(f"  Sample response: {results[0][2][:60].decode()}")
    return total


def test_connection_accept_concurrency():
    """Test if the server accepts multiple connections quickly
    even when processing is serial. Opens N connections simultaneously,
    sends request lazily, measures connect time vs response time."""
    import socket
    print(f"\n[Connect-only] {N} TCP connections, no request sent...")
    t0 = time.perf_counter()
    socks = []
    for _ in range(N):
        s = socket.create_connection(("127.0.0.1", 8080), timeout=5)
        socks.append(s)
    connect_time = time.perf_counter() - t0
    print(f"  Connect all {N}: {connect_time*1000:.1f} ms")
    for s in socks:
        s.close()
    return connect_time


if __name__ == "__main__":
    print("=" * 60)
    print("1y HTTP Server Concurrency Benchmark")
    print("=" * 60)

    # Warmup
    print("\n[Warmup] Single request...")
    try:
        status, t, body = fetch()
        print(f"  Status: {status}, Time: {t*1000:.1f} ms")
        print(f"  Body: {body[:60].decode()}")
    except Exception as e:
        print(f"  FAILED: {e}")
        print("  Is the server running? Start it with:")
        print("  cargo run -- run examples/http_server.1y")
        exit(1)

    # Baseline single
    print("\n[Baseline] Single request (avg of 3)...")
    times = []
    for _ in range(3):
        _, t, _ = fetch()
        times.append(t)
    baseline = sum(times) / len(times)
    print(f"  Avg: {baseline*1000:.1f} ms")

    # Run tests
    seq_time = test_sequential()
    conn_time = test_connection_accept_concurrency()
    conc_time = test_concurrent()

    # Analysis
    print("\n" + "=" * 60)
    print("Analysis")
    print("=" * 60)
    print(f"  Baseline (1 req):     {baseline*1000:7.1f} ms")
    print(f"  Sequential ({N} req):  {seq_time*1000:7.1f} ms  (ratio: {seq_time/baseline:.1f}x)")
    print(f"  Concurrent ({N} req):  {conc_time*1000:7.1f} ms  (ratio: {conc_time/baseline:.1f}x)")
    print(f"  Connect {N} sockets:   {conn_time*1000:7.1f} ms")

    if conc_time > seq_time * 0.8:
        print("\n  >> CONCLUSION: Requests are processed SERIALLY.")
        print("     (Expected: 1y handler is synchronous, no async/await yet)")
        print("     Connection accept is non-blocking, but handler execution blocks.")
    elif conc_time < seq_time * 0.5:
        print("\n  >> CONCLUSION: Requests are processed CONCURRENTLY!")
    else:
        print("\n  >> CONCLUSION: Partial concurrency detected.")
