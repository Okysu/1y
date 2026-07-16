---
title: Concurrent Web Crawler
---

# Concurrent Web Crawler

A web crawler is a textbook I/O-bound task: most of the time is spent waiting for remote servers to respond, and a single-threaded sequential crawl would accumulate all those waits. 1y's `parallel` module dispatches multiple crawl tasks to different worker threads, each blocking independently, so a slow site does not delay the fast ones. This example uses `parallel.map` for batch concurrent crawling and `parallel.spawn` + `parallel.join` for finer-grained asynchronous launch, relying only on the standard library's `tls` module with no external HTTP client dependency.

## The fetch_page Function

What runs on each worker thread is just an ordinary TLS `GET` request. Because `parallel` assigns each task its own worker thread, the blocking calls here are safe — one thread blocking on a slow site does not stop the others from working:

```1y
import tls;

fn fetch_page(host, path) -> Str {
    try {
        let stream = tls.connect(host, 443);
        let request =
            "GET " + path + " HTTP/1.1\r\n" +
            "Host: " + host + "\r\n" +
            "User-Agent: 1y-crawler/0.1\r\n" +
            "Connection: close\r\n" +
            "\r\n";
        tls.write(stream, request);
        let response = "";
        loop {
            let chunk = tls.read(stream, 4096);
            match chunk {
                s if is_str(s) => response = response + s,
                nil => break response
            }
        }
    } rescue as e {
        "[error] " + str(e)
    }
}
```

A few notes:

- **`tls.connect(host, 443)`** completes the TLS handshake and returns an encrypted stream. The `tls` module is built on `rustls` with the Mozilla root certificates baked in, and the handshake validates the certificate chain.
- **The request text** is a hand-assembled HTTP/1.1 message. `Connection: close` tells the server to close the connection after sending the response, so we can read all the way to EOF to get the full body without parsing `Content-Length`.
- **`loop { ... break response }`** is the 1y idiom for "read until EOF": `tls.read` returns `nil` when the connection closes, at which point `break response` carries the accumulated response out of the loop as the value of the whole `loop` expression.
- **`try`/`rescue`** catches network exceptions and returns a string starting with `[error]`, so a single failure does not bring down the entire crawl.

## Analysis Functions

Once the response is fetched, the rest is pure computation — skip the HTTP headers, count words. These functions have no I/O and are safe to run on the worker thread:

```1y
// Skip the HTTP status line + headers (everything before the blank line).
fn extract_body(response) -> Str {
    let parts = split(response, "\r\n\r\n");
    if count(parts) > 1 { parts[1] } else { response }
}

// Count word-like tokens separated by whitespace.
fn count_words(text) -> Int {
    let spaces = split(text, " ");
    let newlines = split(text, "\n");
    count(spaces) + count(newlines)
}
```

`extract_body` splits the response on `\r\n\r\n` (the blank line) into "status line + headers" and "body," and returns the body; if the split produces only one part (e.g. an error response), it returns the whole thing as the body. `count_words` is a rough word estimate, splitting on spaces and newlines and counting the fragments — crude but sufficient.

Combining fetch and analysis into one function returns a Map with the results:

```1y
// Combined: fetch + analyze, returns a Map with results.
fn fetch_and_count(host, path) -> Map {
    let response = fetch_page(host, path);
    let body = extract_body(response);
    let words = count_words(body);
    {
        "host": host,
        "path": path,
        "status": "ok",
        "words": words,
        "bytes": len(body)
    }
}
```

The benefit of bundling fetch and analysis is that the worker thread processes the raw response locally and the main thread only receives a small, tidy Map, without having to transport the entire response text across threads.

## Batch Crawl with parallel.map

`parallel.map` takes a function name and a list of arguments, dispatches each argument to a different worker thread to run concurrently, and returns the list of results once all tasks finish. Here we treat three URLs as arguments and crawl them concurrently:

```1y
println("=== Concurrent Web Crawler ===");
println("CPU cores: " + str(parallel.cores()));
println("");

let urls = [
    ["example.com", "/"],
    ["example.org", "/"],
    ["example.net", "/"]
];

// --- Approach A: parallel.map (batch) ---
println("fetching " + str(count(urls)) + " pages via parallel.map...");
let results = parallel.map("fetch_and_count", urls);

let i = 0;
while i < count(results) {
    let r = results[i];
    println("  " + r["host"] + r["path"] + " — " + str(r["words"]) + " words, " + str(r["bytes"]) + " bytes");
    i += 1
};
```

A few notes:

- **`parallel.cores()`** returns the number of available CPU cores, helping you judge the upper bound of concurrency.
- **`parallel.map("fetch_and_count", urls)`** dispatches each `[host, path]` in `urls` to a worker thread as arguments to `fetch_and_count(host, path)`. The three TLS connections are initiated simultaneously on different threads, each blocking independently.
- **Results are returned in order** — `results[i]` corresponds to `urls[i]`, even if some task finishes early; its position in the result list does not change.
- **A slow site does not delay fast ones**: if `example.org` is slow to respond, `example.com` and `example.net` still finish their own work — only the overall return time of `parallel.map` is affected by the slowest task.

This "throw a batch of independent I/O tasks at `parallel.map`" pattern is the most common use of the `parallel` module — concise, concurrent, and ordered.

## parallel.spawn + join

Sometimes you do not want to throw all tasks out at once but rather start tasks at different points in the code and collect their results later. `parallel.spawn` starts a task asynchronously and returns a handle immediately; `parallel.join` uses the handle to wait for the task to finish and fetch the result:

```1y
// --- Approach B: parallel.spawn + parallel.join (individual) ---
// Useful when you want to kick off tasks at different points in the code
// and collect results later.
println("fetching 2 more pages via parallel.spawn + join...");

let h1 = parallel.spawn("fetch_and_count", ["jsonplaceholder.typicode.com", "/posts/1"]);
let h2 = parallel.spawn("fetch_and_count", ["jsonplaceholder.typicode.com", "/posts/2"]);

// ... do other work here while workers fetch in background ...

let r1 = parallel.join(h1);
let r2 = parallel.join(h2);

println("  " + r1["host"] + r1["path"] + " — " + str(r1["bytes"]) + " bytes");
println("  " + r2["host"] + r2["path"] + " — " + str(r2["bytes"]) + " bytes");
```

`parallel.spawn` returns immediately while the worker thread starts fetching in the background; the main thread can go on to do other work. When you actually need the result, call `parallel.join` — if the task has not finished yet it blocks and waits, and if it has finished it returns immediately. This "spawn — do other work — join" pattern is more flexible than `parallel.map` and suits scenarios where task start times are not fixed.

## When to Use parallel vs Actor

1y has two concurrency toolkits — `parallel` (multi-threaded) and Actor (single-threaded event loop). Facing an I/O task, which should you pick? The key criterion is: **does the I/O operation you are calling have an async variant?**

| Dimension | `parallel` (multi-threaded) | Actor + `task_all` (single-threaded event loop) |
|-----------|------------------------------|--------------------------------------------------|
| Runtime model | One OS worker thread per task, each blocking independently | All Actors share one event loop, cooperatively scheduled |
| I/O style | Blocking I/O (`tls.connect`, `tls.read`) | Non-blocking async I/O (`socket.read_async`) |
| Best for | Libraries without an async variant (e.g. `tls`), CPU-bound work | Libraries with an async variant, tens of thousands of concurrent connections |
| Effect of one blocking task | Only its own thread blocks; other threads keep going | The whole event loop stalls, freezing all Actors |
| Context switching | OS thread switches have cost, task count bounded by thread pool | Cooperative scheduling, near-zero switching cost, scales to very high concurrency |
| Programming model | Ordinary function calls, synchronous return | Message passing + `task_all` to await multiple async results |

Rules of thumb:

- **If the library you call has no async variant (like `tls`), use `parallel`.** `tls.connect` and `tls.read` are blocking — putting them in an Actor event loop would freeze the whole loop, but putting them on a `parallel` worker thread only blocks that one thread, leaving the others working. This example is exactly that situation: the `tls` module has no async variant yet, so `parallel` is the natural choice.
- **For CPU-bound tasks** (parsing, compression, encryption), also use `parallel`.** Such tasks have no I/O wait and are pure CPU work; multiple threads can genuinely run in parallel across multiple cores.
- **If the library you call has an async variant (like `socket.read_async`), use Actor + `task_all`.** A single event loop can host tens of thousands of async tasks without any single slow connection blocking a whole thread; `task_all` awaits a group of async tasks concurrently, semantically similar to `parallel.map` but running on a single thread.

In short: **blocking I/O reaches for `parallel`, async I/O reaches for Actor.** This example uses `parallel` because the `tls` module is blocking, and the code reads as plain function calls with no callbacks or promises — it is just that those calls happen to run on other threads. That is the design goal of the `parallel` module: making "run a function on another thread" almost indistinguishable from "call a function."
