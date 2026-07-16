---
title: 并发 Web 爬虫
---

# 并发 Web 爬虫

网络爬虫是典型的 I/O 密集型任务:大部分时间花在等待远端服务器响应,单线程串行抓取会把这些等待累加起来。1y 的 `parallel` 模块把多个抓取任务分派到不同的工作线程上,每个线程独立阻塞,慢站点不会拖累快站点。本例用 `parallel.map` 做批量并发抓取,用 `parallel.spawn` + `parallel.join` 做更细粒度的异步启动,全程只靠标准库的 `tls` 模块,没有任何外部 HTTP 客户端依赖。

## fetch_page 函数

每个工作线程上跑的就是一次普通的 TLS `GET` 请求。因为 `parallel` 给每个任务分配了独立的工作线程,所以这里的阻塞调用是安全的——一个线程阻塞在慢站点上,其他线程照常工作:

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

几处要点:

- **`tls.connect(host, 443)`** 完成 TLS 握手并返回一个加密流。`tls` 模块基于 `rustls`,内置 Mozilla 根证书,握手过程中会校验证书链。
- **请求文本** 是手工拼的 HTTP/1.1 报文。`Connection: close` 让服务器发完响应就关连接,这样我们可以一直读到 EOF 拿到完整响应体,不必解析 `Content-Length`。
- **`loop { ... break response }`** 是 1y 里"读到 EOF"的惯用法:`tls.read` 返回 `nil` 表示连接关闭,此时 `break response` 把累积的响应带出循环作为整个 `loop` 表达式的值。
- **`try`/`rescue`** 捕获网络异常,返回一个以 `[error]` 开头的字符串,而不是让单个失败拖垮整个爬取过程。

## 分析函数

抓到响应之后是纯计算——跳过 HTTP 头、统计字数。这些函数没有任何 I/O,可以放心地放在工作线程上跑:

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

`extract_body` 用 `\r\n\r\n`(空行)把响应切成"状态行 + 头部"和"响应体"两段,取第二段;如果切不开(比如出错响应),就把整段当成响应体返回。`count_words` 是一个粗略的字数估计,按空格和换行切分后数片段数——简单但够用。

把抓取和分析组合成一个函数,返回一个包含结果的 Map:

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

把抓取和分析捆在一起的好处是:工作线程拿到原始响应后立刻在本地处理,主线程只接收已经整理好的小 Map,不必搬运整段响应文本。

## parallel.map 批量爬取

`parallel.map` 接受一个函数名和一组参数,把每个参数分派到不同工作线程上并发执行,等所有任务完成后返回结果列表。这里把三个 URL 当作参数,并发抓取:

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

几点说明:

- **`parallel.cores()`** 返回可用的 CPU 核心数,帮你判断并发度的上限。
- **`parallel.map("fetch_and_count", urls)`** 把 `urls` 里的每个 `[host, path]` 当作参数,分派到工作线程上调用 `fetch_and_count(host, path)`。三个 TLS 连接在不同线程上同时发起,彼此独立阻塞。
- **结果是按顺序返回的**——`results[i]` 对应 `urls[i]`,即便某个任务先完成,它在结果列表里的位置也不变。
- **慢站点不影响快站点**:如果 `example.org` 响应慢,`example.com` 和 `example.net` 仍然会先完成自己的工作,只是 `parallel.map` 整体返回时间受最慢的那个影响。

这种"把一批独立的 I/O 任务丢给 `parallel.map`"的模式是 `parallel` 模块最常用的用法——简洁、并发、结果有序。

## parallel.spawn + join

有时候你不想一次性把所有任务批量丢出去,而是想在代码的不同位置各自启动一个任务,稍后再统一收集结果。`parallel.spawn` 异步启动一个任务并立即返回一个句柄,`parallel.join` 用句柄等待任务完成并取回结果:

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

`parallel.spawn` 调用立刻返回,工作线程在后台开始抓取,主线程可以继续做别的事;等到真的需要结果时再 `parallel.join`,如果任务还没完成就阻塞等待,如果已完成则立即返回。这种"启动—做别的—等待"的模式比 `parallel.map` 更灵活,适合任务启动时机不固定的场景。

## 何时用 parallel vs Actor

1y 有两套并发工具——`parallel` 多线程和 Actor 单线程事件循环。面对一个 I/O 任务,该选哪个?核心判据是:**你调用的 I/O 操作有没有 async 版本**。

| 维度 | `parallel`(多线程) | Actor + `task_all`(单线程事件循环) |
|------|----------------------|--------------------------------------|
| 运行模型 | 每个任务一个 OS 工作线程,彼此独立阻塞 | 所有 Actor 共享一个事件循环,任务在循环上协作调度 |
| I/O 风格 | 阻塞 I/O(`tls.connect`、`tls.read`) | 非阻塞 async I/O(`socket.read_async`) |
| 适合场景 | 调用没有 async 版本的库(如 `tls`)、CPU 密集型 | 调用有 async 版本的库、需要成千上万个并发连接 |
| 单任务阻塞影响 | 只阻塞自己那个线程,其他线程不受影响 | 阻塞整个事件循环,所有 Actor 都卡住 |
| 上下文切换 | OS 线程切换有成本,任务数受限于线程池 | 协作式调度,切换成本极低,可支撑极高并发 |
| 编程模型 | 普通函数调用,同步返回 | 消息传递 + `task_all` 等待多个 async 完成 |

经验法则:

- **如果调用的库没有 async 版本(比如 `tls`),就用 `parallel`**。`tls.connect` 和 `tls.read` 都是阻塞的,放进 Actor 事件循环里会卡死整个循环;放进 `parallel` 工作线程里则只是阻塞那一个线程,其他线程照常工作。本例正是这种情况——`tls` 模块尚无 async 变体,`parallel` 是自然的选择。
- **如果是 CPU 密集型任务**(解析、压缩、加密),也用 `parallel`**。这类任务没有 I/O 等待,纯靠 CPU 算,多线程能真正并行利用多核。
- **如果调用的库有 async 版本(比如 `socket.read_async`),就用 Actor + `task_all`**。单个事件循环上可以挂成千上万个 async 任务,而不会因为某个慢连接阻塞整条线程;`task_all` 用来并发等待一组 async 任务完成,语义类似于 `parallel.map` 但跑在单线程上。

简而言之:**阻塞 I/O 用 `parallel`,async I/O 用 Actor**。本例因为 `tls` 模块是阻塞的,所以选了 `parallel`,代码读起来就是普通的函数调用,没有任何回调或 Promise——只是这些调用恰好跑在了别的线程上。这正是 `parallel` 模块的设计目标:让"在另一条线程上跑一个函数"这件事,简单到和"调用一个函数"几乎没区别。
