---
title: TLS HTTP 客户端
---

# TLS HTTP 客户端

绝大多数现代网络请求都跑在 HTTPS 上,这意味着任何一门想接入真实世界的语言都得能完成 TLS 握手。1y 的 `tls` 模块基于 `rustls`,内置 Mozilla 根证书,`connect` 一调用就完成了证书验证——你拿到的就是一个可以读写的加密流。本例从零写一个最小可用的 HTTPS 客户端:发起 `GET` 请求、读取响应、解析状态行,并用 `try`/`rescue` 处理网络错误。由于 1y 不提供高层 HTTP 客户端,这个例子也能帮你理解 HTTP/1.1 协议的最小骨架。

## 建立 TLS 连接

一切从 `tls.connect` 开始:

```1y
import tls;

let host = "example.com";
let stream = tls.connect(host, 443);
println("connected to: " + tls.peer_addr(stream));
```

`tls.connect(host, 443)` 会执行完整的 TLS 握手,包括域名验证和证书链校验。**握手是急切的(eager)**——如果证书有问题,`connect` 会直接抛出异常,而不是等到第一次读写时才失败。这意味着:只要这行成功返回,你就拥有了一条可信任的加密通道。`tls.peer_addr(stream)` 返回对端地址,方便调试。注意 `stream` 是一个不透明(opaque)的资源句柄,你只能通过 `tls` 模块的函数操作它。

## 构造 HTTP 请求

HTTP/1.1 的请求本质上就是一段符合格式的文本。我们手工拼一个 `GET` 请求:

```1y
fn build_get_request(host, path) -> Str {
    "GET {path} HTTP/1.1\r\n" +
    "Host: {host}\r\n" +
    "User-Agent: 1y-http-demo/0.1\r\n" +
    "Connection: close\r\n" +
    "\r\n"
}
```

让我们逐行拆解这段请求文本:

- **请求行** `GET {path} HTTP/1.1\r\n` 声明方法、路径和协议版本。这里的 `{path}` 是字符串插值,运行时会被替换成实参。`\r\n` 是 HTTP 规定的行结束符(CRLF),Windows 风格的换行,千万别写成 `\n`。
- **`Host` 头** 在 HTTP/1.1 里是必填的,因为一台服务器可能用虚拟主机托管多个域名,需要靠它区分。
- **`User-Agent` 头** 标识客户端身份。许多服务器会拒绝缺少 `User-Agent` 的请求,加上它是个好习惯。
- **`Connection: close`** 告诉服务器:响应发完就关连接。这让我们可以一直读到 EOF 来获取完整响应体,而不必解析 `Content-Length`。
- **空行 `\r\n`** 标志请求头结束,服务器看到它才开始处理。

这里用 `+` 拼接字符串纯粹是为了可读性;你也可以把整段请求写进一个三引号字符串里。插值 `{path}` 让函数能复用于任意路径。

## 发送请求并读取响应

有了请求文本,把它写进流,再循环读取响应:

```1y
let request = build_get_request(host, "/");
tls.write(stream, request);

let response = "";
loop {
    let chunk = tls.read(stream, 4096);
    match chunk {
        s if is_str(s) => response = response + s,
        nil => break response
    }
};
```

几处要点:

- **`tls.write(stream, request)`** 把字符串写入加密流,内容会被 TLS 层加密后发出。返回 `Nil`,不返回字节数。
- **`tls.read(stream, 4096)`** 最多读取 4096 字节,返回一个字符串。当连接关闭(我们设了 `Connection: close`,服务器发完响应就会关闭)时返回 `nil`。
- 我们用 `loop { ... break response }` 反复读取并拼接。`loop` 是无限循环,`break response` 退出循环并返回 `response` 作为整个 `loop` 表达式的值。这是 1y 里"读直到 EOF"的惯用法。
- 当 `chunk` 为 `nil` 时,我们 `break response`,把累积的完整响应带出循环。

## 解析状态行

响应的第一行是状态行,形如 `HTTP/1.1 200 OK`。我们把它切出来:

```1y
let lines = split(response, "\r\n");
let status_line = first(lines);
let parts = split(status_line, " ");
let version = parts[0];
let code = parts[1];
let reason = parts[2];

println("version: " + version);
println("status:  " + code + " " + reason);
```

`split(response, "\r\n")` 按行切分;`first` 取第一行;再按空格切成三段——协议版本、状态码、原因短语。`parts[1]` 是状态码字符串,如 `"200"`。在生产代码里,你应当检查 `code` 是否以 `2` 开头来判断成功,而不是假设请求一定如愿。

## 用 try/rescue 处理错误

网络是不可靠的:DNS 解析失败、证书过期、连接被重置……这些都会以异常的形式抛出。1y 用 `try`/`rescue` 捕获:

```1y
import tls;

fn fetch(host, path) -> Str {
    try {
        let stream = tls.connect(host, 443);
        tls.write(stream, build_get_request(host, path));
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

println(fetch("example.com", "/"));
```

`try { ... } rescue as e { ... }` 捕获任意异常,把异常值绑定到 `e`。`rescue` 分支也是一个表达式,它的值会成为整个 `try` 的返回值——因此当出错时,`fetch` 返回一个以 `[error]` 开头的字符串,而非让程序崩溃。这种"用异常值而非错误码"的风格让错误处理和正常逻辑共享同一套表达式机制。在生产环境,你还可以用模式匹配的 `rescue Pattern as e` 只捕获特定异常,把其余的继续向外抛。

## 小结

短短几十行,我们就实现了一个能用的 HTTPS 客户端。这个例子的价值不在于"又造了一个 HTTP 库",而在于展示了 1y 如何把底层能力(TLS、socket、字符串)交给程序员:没有隐式的运行时魔法,请求是你亲手拼的文本,响应是你亲手读的字节,错误是你亲手处理的异常。当你日后使用更高级的库时,理解这层底层模型会让你对网络编程有更扎实的把握。最后别忘了清理资源——`tls.close(stream)` 关闭流,虽然在短脚本里程序退出也会释放,但在长跑的服务里显式关闭是好习惯。
