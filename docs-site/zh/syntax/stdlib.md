---
title: 标准库概览
---

# 标准库概览

1y 内置 **10 个标准库模块**，覆盖环境变量、文件 I/O、JSON、进程控制、随机数、串口、TCP 网络、加密、TLS 与动态库加载。所有模块都通过 `import` 引入，提供命名空间下的函数调用。本页给出每个模块的用途与关键函数，帮助你快速定位所需能力；各函数的详细签名可查阅语言参考。

## 模块总览

| 模块 | 用途 | 关键函数 |
|------|------|----------|
| `env` | 环境变量 | `get`, `set`, `unset`, `args`, `vars` |
| `io` | 文件 I/O | `read_line`, `read_to_string`, `write`, `append`, `exists` |
| `json` | JSON 编解码 | `parse`, `stringify`, `pretty` |
| `process` | 进程控制 | `exit`, `exec`, `exec_status`, `pid`, `cwd`, `set_cwd`, `sleep_ms`, `sleep_async` |
| `random` | 伪随机数（xorshift64） | `int`, `range`, `float`, `bool`, `pick`, `shuffle`, `seed` |
| `serial` | 串口通信 | `open`, `list`, `read`, `write`, `close` |
| `socket` | TCP 网络 | `listen`, `accept`, `connect`, `read`, `read_line`, `write`, `close`, `set_nonblocking`, `peer_addr`, `read_async` |
| `crypto` | 哈希与 CSPRNG | `sha256`, `sha512`, `sha1`, `md5`, `hmac_sha256`, `hmac_sha512`, `base64_encode/decode`, `hex_encode/decode`, `random_bytes`, `secure_int`, `secure_float` |
| `tls` | TLS 客户端（rustls） | `connect`, `read`, `read_line`, `write`, `close`, `peer_addr` |
| `ffi` | 动态库加载 | `load`, `call`, `unload`, `is_loaded` |

> **Task 组合子**（`task_all`、`task_any`、`task_ready`）是全局内置函数，不属于任何模块——见 [Tasks](#tasks)。

## env — 环境变量

`env` 读取与修改进程的环境变量，并访问命令行参数。

```1y
import env;

let home = env.get("HOME");
env.set("MODE", "debug");
env.unset("TEMP");

let argv = env.args();        // 命令行参数列表
let all = env.vars();         // 当前所有环境变量
```

`args` 返回启动时的参数向量，`vars` 返回所有环境变量，便于配置发现与诊断。`set`/`unset` 只影响当前进程及其子进程，不会回写到父 shell。

## io — 文件 I/O

`io` 提供文本与字节层面的文件读写，以及存在性检查。

```1y
import io;

let text = io.read_to_string("config.txt");
io.write("log.txt", "启动完成");
io.append("log.txt", "第二步完成");

if io.exists("data.bin") {
    let line = io.read_line();   // 从标准输入读一行
    print("-" + line);
}
```

`read_line` 从标准输入读取一行，`read_to_string` 一次性读入整个文件为字符串。`write` 覆盖写，`append` 追加写。文件不存在时 `exists` 返回 `false`，而 `read_to_string` 会报错——先用 `exists` 探测，或用 `match`/`try` 捕获错误。

## json — JSON 编解码

`json` 在 1y 值与 JSON 文本之间双向转换。

```1y
import json;

let obj = { name: "1y", version: 1 };
let compact = json.stringify(obj);
let pretty = json.pretty(obj);   // 带缩进，便于阅读

let parsed = json.parse(compact);
```

`parse` 把 JSON 文本解析回 1y 值（对象变 Map、数组变 Vec、数字保持数值统一类型），`stringify` 生成紧凑文本，`pretty` 生成带缩进的易读文本。`parse` 遇到非法 JSON 会报错，建议在外层做错误处理。

## process — 进程控制

`process` 管理当前进程：退出、执行子命令、查询身份、切换目录、休眠。

```1y
import process;

let code = process.exec_status("ls", ["-la"]);
process.exec("echo", ["hello"]);

process.set_cwd("/tmp");
let dir = process.cwd();
let me = process.pid();

process.sleep_ms(500);     // 休眠 500 毫秒
process.exit(0);
```

`exec` 阻塞执行子命令并等待完成，`exec_status` 返回退出码，`exit` 立即终止当前进程。`sleep_ms` 让当前执行流休眠指定毫秒数——在事件循环模型下，它会交出调度权，让其他 Actor 有机会运行。`sleep_async(ms)` 返回一个 `Task<Nil>`，在 `ms` 毫秒后完成；`await process.sleep_async(ms)` 挂起当前协程而不阻塞其他 Actor（这是无色异步用来保证慢路由不拖累事件循环的核心原语）。

## random — 伪随机数

`random` 基于 xorshift64 算法，提供快速但**非密码学安全**的伪随机数。

```1y
import random;

random.seed(42);
let n = random.int(100);            // [0, 100)
let r = random.range(1, 100);      // [1, 100)
let f = random.float();            // [0.0, 1.0)
let b = random.bool();
let choice = random.pick([1, 2, 3]);
let shuffled = random.shuffle([1, 2, 3, 4]);
```

需要密码学安全的随机数请改用 `crypto` 模块的 `random_bytes` / `secure_int` / `secure_float`。`seed` 用于固定序列，便于测试复现；不调用 `seed` 时使用默认种子。

## serial — 串口通信

`serial` 用于与串口设备交互，常见于嵌入式与工业场景。

```1y
import serial;

let ports = serial.list();                 // 枚举可用串口
let dev = serial.open("COM3", 115200);     // 或 /dev/ttyUSB0
serial.write(dev, "PING");
let data = serial.read(dev, 64);
serial.close(dev);
```

`open` 接受设备名与波特率，返回串口句柄；`read` 的第二个参数是期望读取的最大字节数。串口通信通常是阻塞的，注意在事件循环中合理使用。

## socket — TCP 网络

`socket` 提供阻塞式 TCP 服务端与客户端能力，并可切换非阻塞模式。

```1y
import socket;

let listener = socket.listen("127.0.0.1", 8080);
let conn = socket.accept(listener);
socket.set_nonblocking(conn, true);
let line = socket.read_line(conn);
socket.write(conn, "HTTP/1.1 200 OK");
print(socket.peer_addr(conn));
socket.close(conn);

// 客户端
let c = socket.connect("example.com", 80);
socket.write(c, "GET / HTTP/1.0");
socket.close(c);
```

`listen` 创建监听套接字，`accept` 阻塞等待并返回新连接。`set_nonblocking` 让后续 `read`/`read_line` 在无数据时立即返回而非挂起，便于与事件循环配合实现并发服务。`peer_addr` 返回对端地址，便于日志与鉴权。`read_async(stream, n)` 返回一个 `Task<Str|Nil>`，在流上最多有 `n` 字节可读时完成；`await socket.read_async(stream, 65536)` 挂起协程，直到 OS 报告该流可读（经 `mio`），从而一个慢连接永远不会阻塞其他连接。

## Tasks — 异步组合

`await` 是核心挂起原语（见[无色异步](../philosophy/no-async.md)）。`Task` 是由异步 I/O 函数（`socket.read_async`、`process.sleep_async`）或下列全局组合子产生的值。Task 是**一次性**的：`await` 会消费它。你可以在任何函数体里 `await`——不需要 `async` 标记。

| 函数 | 签名 | 说明 |
|------|------|------|
| `task_ready` | `task_ready(value) -> Task` | 一个已经以 `value` 完成的 Task。 |
| `task_all` | `task_all([t1, t2, ...]) -> Task<Vec<value>>` | **全部**输入完成时完成，结果按顺序排列。成功时消费所有输入。 |
| `task_any` | `task_any([t1, t2, ...]) -> Task<value>` | **任一**输入完成时完成，返回最先就绪的值。仅消费获胜的输入。 |

```1y
import process;

// 把一个普通值包装成 Task
let now = await task_ready(42);

// 并发跑两个休眠，等两者都完成
let both = await task_all([
    process.sleep_async(100),
    process.sleep_async(150)
]);
println(str(count(both)));    // 2

// 让两个 Task 竞速，快的赢
let winner = await task_any([
    process.sleep_async(100),
    process.sleep_async(500)
]);
```

长生命周期的并发状态（计数器、缓存、会话）请用 **Actor**（`spawn Name(args)`）；`Task` 用于组合异步 I/O，不用于共享可变状态。

## crypto — 哈希与 CSPRNG

`crypto` 提供摘要算法、HMAC、编码与密码学安全随机数。

```1y
import crypto;

let h = crypto.sha256("hello");
let mac = crypto.hmac_sha256("secret", "payload");
let b64 = crypto.base64_encode(raw_bytes);
let hex = crypto.hex_encode(raw_bytes);

let token = crypto.random_bytes(32);
let dice = crypto.secure_int(1, 6);
let rnd = crypto.secure_float();
```

支持 `sha256`/`sha512`/`sha1`/`md5` 摘要，`hmac_sha256`/`hmac_sha512` 消息认证码，`base64`/`hex` 编解码，以及 CSPRNG。**不要用 `md5`/`sha1` 做安全用途**——它们已被证明存在碰撞攻击，仅适合校验和等非安全场景。

## tls — TLS 客户端

`tls` 基于 rustls 提供安全的 TLS 客户端连接，接口与 `socket` 相似但自带加密与证书校验。

```1y
import tls;

let conn = tls.connect("example.com", 443);
tls.write(conn, "GET / HTTP/1.1");
let line = tls.read_line(conn);
print(tls.peer_addr(conn));
tls.close(conn);
```

`connect` 完成完整的 TLS 握手（包括证书链验证），握手失败会报错。一旦建立，`read`/`write` 在加密通道上进行，你无需关心加解密细节。`tls` 目前定位为客户端，适用于发起 HTTPS 请求等场景。

## ffi — 动态库加载

`ffi` 允许加载动态库并调用其中的 C 函数。这是 1y 与原生生态打通的桥梁，详见 [FFI 外部函数接口](./ffi)。

```1y
import ffi;

let lib = ffi.load("libc.so.6");
let r = ffi.call(lib, "abs", "int(int)", [-42]);
ffi.unload(lib);
```

由于 FFI 跨越了 1y 的安全边界，使用时需格外谨慎，务必保证签名准确、库来源可信。

## 导入与使用

标准库模块通过 `import` 引入，函数以 `模块名.函数名` 形式调用：

```1y
import io;
import json;
import crypto;

io.write("out", json.pretty({ salt: crypto.random_bytes(16) }));
```

模块采用**延迟导入**：首次使用时才真正加载，且整个程序中同一模块只加载一次。你也可以为模块取别名以缩短调用：

```1y
import crypto as c;
let h = c.sha256("x");
```

标准库刻意保持精简——只收录跨平台、无外部依赖的基础能力。更专用的功能（数据库驱动、HTTP 框架等）留给生态以第三方包形式提供，而与操作系统底层的交互则通过 `ffi` 直接调用动态库实现。

## 全局内置函数

除了上述 10 个模块，1y 还内置一组**全局函数**——无需 `import` 即可调用。完整说明见 [反射与动态求值](./introspection)，下面按用途分类给出索引。

### I/O 与打印

| 函数 | 作用 |
|------|------|
| `println(v)` / `print(v)` | 打印值到 stdout（println 带换行） |

### 集合操作

| 函数 | 作用 |
|------|------|
| `count(coll)` | 元素数量（Vec/Map/Set/Str） |
| `first(coll)` / `rest(coll)` | 首元素 / 去掉首元素后的集合 |
| `cons(x, xs)` / `push(xs, x)` | 头部 / 尾部追加 |
| `get(coll, k)` / `has_key(m, k)` | 索引取值 / Map 键存在性 |
| `assoc(m, k, v)` / `dissoc(m, k)` | Map 添加 / 删除键 |
| `iter_to_vec(iterable)` | 任意可迭代值物化为 Vec |
| `keys(m)` / `values(m)` / `fields(struct)` | Map/Struct 的键 / 值 / 字段对 |

### 类型谓词与反射

| 函数 | 作用 |
|------|------|
| `is_int` / `is_decimal` / `is_str` / `is_bool` / `is_nil` / `is_vec` / `is_map` / `is_set` / `is_number` / `is_func` / `is_closure` | 类型判断 |
| `type_of(v)` | 类型名字符串 |
| `instance_of(v, name)` | 类型名匹配（带 Str↔String、Func↔Closure 规范化） |
| `variant_name(v)` / `variant_args(v)` | Variant 构造器名 / 携带参数 |
| `ast_of(src)` | 把源码字符串解析成 AST 数据 |
| `eval(src)` | 动态求值源码字符串 |

### 算术与数值

| 函数 | 作用 |
|------|------|
| `pow(a, b)` / `abs(n)` | 幂 / 绝对值 |
| `min(a, b)` / `max(a, b)` | 较小 / 较大值 |
| `floor` / `ceil` / `round` / `sqrt` / `sin` / `cos` / `log` / `exp` | 数学函数 |
| `to_i64(v)` / `to_f64(v)` / `int(v)` / `decimal(v)` | 数值转换 |

### 字符串

| 函数 | 作用 |
|------|------|
| `len(s)` / `split(s, sep)` / `join(xs, sep)` | 长度 / 分割 / 拼接 |
| `replace(s, a, b)` / `trim(s)` / `contains(s, sub)` | 替换 / 去空白 / 包含 |
| `substring(s, start, end)` | 子串 |
| `starts_with` / `ends_with` / `index_of` / `char_at` / `codepoint_at` / `from_codepoint` | 其他字符串操作 |
| `byte_at(s, i)` / `byte_len(s)` | 字节级访问 |
| `to_lower(s)` / `to_upper(s)` | 大小写转换 |
| `is_digit(c)` / `is_alpha(c)` / `is_space(c)` | 字符分类 |

### 高阶函数

| 函数 | 作用 |
|------|------|
| `map(f, xs)` / `filter(pred, xs)` / `fold(f, init, xs)` / `reduce(f, xs)` / `find(pred, xs)` / `each(f, xs)` | 列表变换 |

### 转换

| 函数 | 作用 |
|------|------|
| `str(v)` / `to_str(v)` | 转字符串 |

### Task 组合子

| 函数 | 作用 |
|------|------|
| `task_all(tasks)` / `task_any(tasks)` / `task_ready(t)` | 异步组合（见 [Tasks](#tasks)） |

### Actor 内省

| 函数 | 作用 |
|------|------|
| `pid_of(actor)` | 返回 actor 的 Pid（u64） |
