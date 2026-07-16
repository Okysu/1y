---
title: 模块系统
---

# 模块系统

随着程序规模增长,把所有代码塞进一个文件既不利于组织,也不利于复用。1y 的模块系统让你能把代码拆分到多个文件中,再按需引入。模块既是**命名空间**(把一组相关的绑定收纳在一个名字下),也是**复用单元**(一个文件可以导出绑定供其他文件使用)。1y 提供了一组开箱即用的标准库模块,并允许你用同样的机制组织自己的代码。

## import 基础

`import` 语句把一个模块引入当前作用域,并把它绑定到一个名字。最常见的形式是 `import name;`,绑定名就是模块名本身:

```1y
import io;

println("hello");             // println 是全局内置函数
let content = io.read_to_string("data.txt");
```

如果你希望用一个不同的名字来引用模块(例如避免命名冲突,或让名字更贴合语境),可以使用 `import ... as alias`:

```1y
import io as fs;

println("hello via fs");
fs.write("out.txt", "data");
```

`as` 之后的名字会成为当前作用域中该模块的别名,原名则不可用。这在同时引入功能相近的模块时尤其有用。

## lazy import:延迟加载

`lazy import` 把模块的加载推迟到**第一次实际访问**它的时刻。在模块昂贵(例如包含大量初始化)或仅在少数代码路径中需要时,延迟加载能避免启动时不必要的开销:

```1y
lazy import json;

// 此时 json 尚未真正加载

fn maybe_parse(s) {
    if looks_like_json(s) {
        json.parse(s)        // 第一次访问 json,模块在此刻加载
    } else {
        nil
    }
}
```

`import` 与 `lazy import` 的语义区别只在于加载时机:一旦加载完成,两者在使用上完全相同。对于绝大多数轻量模块,直接用 `import` 即可;`lazy import` 适合那些"可能用不到"或"加载有成本"的模块。

## 标准库模块

1y 内置了一批标准库模块,涵盖了常见的基础能力。它们和用户自定义的文件模块使用相同的 `import` 机制,无需额外配置:

| 模块      | 用途                       |
|-----------|----------------------------|
| `env`     | 环境变量                   |
| `io`      | 文件 I/O                   |
| `json`    | JSON 解析与序列化          |
| `process` | 进程控制                   |
| `random`  | 伪随机数(xorshift64,非加密安全) |
| `serial`  | 串口 I/O                   |
| `socket`  | TCP 网络                   |
| `crypto`  | 哈希、HMAC、编码、CSPRNG   |
| `tls`     | TLS 客户端(rustls)        |
| `ffi`     | 动态库加载                 |

```1y
import json;
import random;

let data = json.parse("\{\"name\": \"Alice\"\}");
let n = random.range(1, 100);
```

## 文件模块

除了标准库,你可以把自己的 `.1y` 文件组织成模块。模块路径用点号分隔:`a.b.c` 会被解析为相对于**入口文件所在目录**的 `a/b/c.1y`。

假设你的项目结构如下:

```
main.1y
utils/
  math.1y
  strings.1y
```

在 `main.1y` 中,可以这样引入:

```1y
import utils.math;
import utils.strings as str_utils;

println(utils.math.square(5));    // 调用 utils/math.1y 中的 square
println(str_utils.upper("hi"));   // 用别名调用 strings.1y 中的 upper
```

一个 `.1y` 文件的**顶层绑定**就是它作为模块导出的内容。也就是说,你在文件顶层用 `let`、`fn`、`type`、`enum` 定义的一切,都会成为该模块的成员,供导入方通过 `module.member` 访问。无需显式的 `export` 关键字。

```1y
// utils/math.1y
fn square(x) -> Int { x * x }
fn cube(x) -> Int { x * x * x }
let PI = 3.14
```

导入方即可使用 `utils.math.square`、`utils.math.cube`、`utils.math.PI`。

## 模块缓存与循环依赖

模块按其**规范化路径(canonical path)**缓存:每个模块文件最多被加载和求值一次,之后所有导入它的地方共享同一个模块实例。这保证了全局状态(如模块顶层的可变绑定)的一致性,也避免了重复加载的开销。

```1y
import utils.config;
import utils.config as cfg;   // 同一个模块,config 与 cfg 引用同一实例
```

不过,缓存机制也带来一个约束:**循环导入会报错**。如果模块 A 导入了 B,而 B 又(直接或间接地)导入了 A,1y 在检测到这种循环时会抛出错误,因为无法在两个互相依赖的模块之间确定求值顺序。例如:

```1y
// a.1y
import b;        // b 又会反过来 import a —— 循环!
```

```1y
// b.1y
import a;
```

遇到循环依赖时,正确的做法是**重构**:把两个模块共同依赖的部分提取到第三个模块中,让依赖关系变成有向无环图。`lazy import` 有时也能用来打破加载阶段的循环,但根本上,模块间的依赖应当是单向的。

## 小结

`import` 引入模块并绑定到名字,`as` 提供别名,`lazy import` 延迟到首次访问时加载。标准库模块开箱即用,文件模块按 `a.b.c → a/b/c.1y` 解析,顶层绑定即为导出。模块按规范路径缓存且只加载一次,循环导入会引发错误——这些规则共同保证了模块系统既灵活又可预测,让你能放心地把程序拆分成清晰、可复用的部分。
