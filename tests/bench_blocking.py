"""
Test whether a slow request blocks fast requests.

Sends 1 slow request (/slow, 500ms) and 5 fast requests (/hello, ~10ms)
concurrently. If the server has true request-level concurrency, the fast
requests finish quickly while the slow one runs. If the server is serial,
all fast requests wait for the slow one to finish first.
"""

import threading
import time
import urllib.request

# Bypass system proxy.
urllib.request.install_opener(
    urllib.request.build_opener(urllib.request.ProxyHandler({}))
)

BASE = "http://127.0.0.1:8080"


def fetch(path, label, results):
    """Fetch BASE/path, record (label, start, end, status) in results."""
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(BASE + path, timeout=10) as r:
            body = r.read()
            status = r.status
    except Exception as e:
        status = f"ERR: {e}"
        body = b""
    t1 = time.perf_counter()
    results.append((label, t0, t1, status, body[:40]))


def main():
    print("=" * 60)
    print("Blocking Test: 1 slow + 5 fast concurrent requests")
    print("=" * 60)
    print()
    print("Slow request: GET /slow (handler sleeps 500ms)")
    print("Fast requests: GET /hello (handler ~1ms)")
    print()

    results = []
    threads = []

    # Start the slow request first.
    t_slow = threading.Thread(target=fetch, args=("/slow", "SLOW", results))
    threads.append(t_slow)

    # Then 5 fast requests.
    for i in range(5):
        threads.append(
            threading.Thread(target=fetch, args=("/hello", f"fast{i}", results))
        )

    t_global_start = time.perf_counter()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=15)
    t_global_end = time.perf_counter()

    # Sort by start time.
    results.sort(key=lambda x: x[1])

    print(f"{'Label':<8} {'Start(ms)':>10} {'End(ms)':>10} {'Dur(ms)':>10} {'Status':<10} Body")
    print("-" * 80)
    for label, t0, t1, status, body in results:
        start_ms = (t0 - t_global_start) * 1000
        end_ms = (t1 - t_global_start) * 1000
        dur_ms = (t1 - t0) * 1000
        body_str = body.decode(errors="replace")[:30] if isinstance(body, bytes) else str(body)[:30]
        print(f"{label:<8} {start_ms:>10.1f} {end_ms:>10.1f} {dur_ms:>10.1f} {str(status):<10} {body_str}")

    total_ms = (t_global_end - t_global_start) * 1000
    print(f"\nTotal wall time: {total_ms:.1f} ms")

    # Analysis
    slow_end = [r for r in results if r[0] == "SLOW"][0][2]
    fast_ends = [r[2] for r in results if r[0].startswith("fast")]
    fast_after_slow = sum(1 for fe in fast_ends if fe > slow_end - 0.05)

    print("\n" + "=" * 60)
    print("Analysis")
    print("=" * 60)
    if fast_after_slow >= 4:
        print(f"  >> SERIALLY BLOCKED: {fast_after_slow}/5 fast requests")
        print("     finished AFTER the slow request.")
        print("     The slow handler blocked the event loop — no request-level")
        print("     concurrency. This is the expected behavior for 1y's current")
        print("     sync handler model (like FastAPI's `def` routes, not `async def`).")
    elif fast_after_slow <= 1:
        print(f"  >> TRUE CONCURRENCY: only {fast_after_slow}/5 fast requests")
        print("     finished after the slow request.")
        print("     Fast requests were NOT blocked by the slow handler!")
    else:
        print(f"  >> PARTIAL: {fast_after_slow}/5 fast requests finished after slow.")
        print("     Some concurrency, but the event loop was partially blocked.")


if __name__ == "__main__":
    main()
