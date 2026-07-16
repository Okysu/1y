---
title: FFI 外部函数接口
---

# FFI 外部函数接口

1y 是一门托管语言，但现实世界有大量用 C 编写的库——操作系统接口、硬件驱动、遗留系统。1y 通过 `ffi` 模块提供**外部函数接口（Foreign Function Interface）**：在运行时加载动态库（`.so`/`.dll`/`.dylib`），按签名调用其中的 C 函数，从而与原生生态打通。

本页介绍 `ffi` 的四个核心函数：`load`、`call`、`unload`、`is_loaded`，以及签名格式、ABI 类型与安全注意事项。

## 加载库：load

`ffi.load(path)` 加载指定路径的动态库，返回一个**库句柄**。后续的 `call` 通过该句柄定位函数。

```1y
import ffi;

let lib = ffi.load("libc.so.6");          // Linux
// let lib = ffi.load("msvcrt.dll");       // Windows
// let lib = ffi.load("libSystem.dylib");  // macOS
```

`load` 会在标准搜索路径中查找库（受 `LD_LIBRARY_PATH`/`PATH`/`DYLD_LIBRARY_PATH` 等环境变量影响），也接受绝对或相对路径。若加载失败，`load` 报错并给出原因。同一库重复 `load` 会得到独立的句柄，通常你只需在程序启动时加载一次，长期持有。

## 调用函数：call

`ffi.call(lib, name, signature, args)` 调用库中名为 `name` 的 C 函数。`signature` 描述返回类型与参数类型，`args` 是参数值列表。

```1y
let r = ffi.call(lib, "abs", "int(int)", [-42]);
print(r);    // 42
```

`call` 的工作流程：

1. 按 `name` 在库中查找函数符号，找不到则报错。
2. 按 `signature` 解析返回类型与参数类型，据此在 1y 值与 C ABI 之间做转换。
3. 以 C 调用约定发起调用，把返回值按签名转换回 1y 值。

## 签名格式

签名是一个形如 `"ret(arg1, arg2, ...)"` 的字符串：括号前是返回类型，括号内是逗号分隔的参数类型列表。无参数时写作 `"ret()"`。

```1y
"int(int)"          // 接收一个 int，返回 int
"void(int)"         // 接收一个 int，无返回值
"str(str)"          // 接收 C 字符串，返回 C 字符串
"float(float)"      // 接收 f64，返回 f64
"int(str, int)"     // 接收 (字符串, 整数)，返回整数
```

返回类型 `void` 表示函数无返回值，`call` 此时返回单元值。签名中的空白会被忽略，因此 `"int( int , str )"` 与 `"int(int,str)"` 等价。

## ABI 类型

签名中的类型对应 C 的 ABI，1y 支持五种：

| 签名类型 | C 对应 | 1y 对应 | 说明 |
|----------|--------|---------|------|
| `void` | `void` | 单元 | 仅用于返回类型 |
| `int` | `int64_t` | 整数 | 有符号 64 位 |
| `uint` | `uint64_t` | 整数 | 无符号 64 位 |
| `float` | `double` | 小数 | 64 位浮点 |
| `str` | `char*` | 字符串 | 以空字节结尾的 C 字符串 |

注意 `int`/`uint` 在 1y 中都映射为任意精度整数，但跨越 FFI 边界时会被截断到 64 位；若值超出范围会产生非预期的包装结果，需自行确保范围合法。`str` 在传入时 1y 字符串被转为 UTF-8 的 C 字符串，返回时反向转换——若 C 函数返回的不是合法 UTF-8，转换可能丢失字节。

## 参数数量限制

`ffi.call` **最多支持 6 个参数**。这是 1y 的 FFI 实现为了保证跨平台寄存器/栈传参的简单与一致而设的上限。绝大多数 C 函数的参数数远少于此；若确实需要更多参数，可在 C 侧封装一个参数更少的包装函数，或把参数打包进一个结构体指针，再以 `int(int)` 之类的方式传入指针。

## 卸载与探测：unload / is_loaded

`ffi.unload(lib)` 卸载库句柄，释放底层资源。卸载后该句柄不可再用，调用 `call` 会报错。

`ffi.is_loaded(lib)` 探测句柄对应的库是否仍处于加载状态，返回布尔值，便于在不确定生命周期时安全检查。

```1y
if ffi.is_loaded(lib) {
    ffi.call(lib, "cleanup", "void()", []);
    ffi.unload(lib);
}
```

需要特别注意的是：如果 `call` 返回的 `str` 或将来返回的指针指向库内部静态数据，那么在 `unload` 之后访问这些数据是未定义行为。最稳妥的做法是**先使用完所有结果，再卸载库**。

## 安全性

FFI 是**本质上不安全（unsafe）**的能力。一旦跨越 FFI 边界，1y 的内存安全、类型安全与隔离保证全部失效：

- **签名必须正确**：若你声明的签名与 C 函数实际签名不符（类型错、参数数错），调用可能触发未定义行为、读越界、甚至崩溃。1y 无法在编译期校验签名——它完全相信你写的字符串。
- **只加载可信库**：动态库的代码以完全权限运行，可读写进程内存、调用任意系统调用。绝不要 `load` 不可信来源的库，等同于执行任意代码。
- **生命周期与线程**：库句柄在卸载后不可使用；若 C 函数内部持有跨调用的指针，1y 的 GC 可能在你不察觉时回收对应内存，需要格外小心地管理所有权。
- **平台差异**：库路径、符号可见性、调用约定可能随平台变化，跨平台代码应针对平台分支处理（例如 Linux 用 `libc.so.6`、Windows 用 `msvcrt.dll`）。

正因如此，FFI 应作为**最后手段**：优先看标准库是否已提供等价能力（`crypto`、`socket`、`tls` 等），确实需要时再用 FFI，并尽量把不安全的调用封装在一个小而厚的模块内，对外暴露安全的 1y 接口，把危险隔离在最小范围内。

## 完整示例

```1y
import ffi;

// 跨平台加载 C 标准库并调用 abs / strlen
let lib = ffi.load("libc.so.6");

let absval = ffi.call(lib, "abs", "int(int)", [-42]);
println("abs(-42) = " + str(absval));

let len = ffi.call(lib, "strlen", "uint(str)", ["hello"]);
println("strlen(hello) = " + str(len));

ffi.unload(lib);
```

FFI 让 1y 不被"生态孤岛"困住：任何能用 C 调用的东西，1y 都能调用。但能力越大，责任越大——务必保证签名准确、库可信、调用被妥善封装。当标准库已经覆盖某能力时，优先使用标准库，把 FFI 留给那些别无他法的场景。
