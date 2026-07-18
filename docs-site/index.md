---
layout: home

hero:
  name: "1y"
  text: "一门流式并发函数式编程语言"
  tagline: 字节码虚拟机 · 持久化数据结构 · 模式匹配 · Actor 并发 · 无色异步 · 软件事务内存
  actions:
    - theme: brand
      text: 中文文档
      link: /zh/syntax/getting-started
    - theme: alt
      text: English Docs
      link: /en/syntax/getting-started

features:
  - title: 字节码虚拟机
    details: 默认后端为基于栈的字节码 VM，将 AST 编译为扁平指令序列，调用帧存于堆，深递归不再溢出栈，HTTP 吞吐较 tree-walker 提升 2.7 倍。
  - title: 数值统一
    details: 整数和小数都是任意精度，算术自动提升，不再有溢出和精度丢失的烦恼。
  - title: 持久化集合
    details: Vec、Map、Set 基于结构共享，"修改"集合返回新版本，旧版本仍然可用。
  - title: Actor 并发
    details: 基于 Actor 模型的消息传递并发，每个 Actor 拥有隔离状态，无数据竞争。
  - title: 无色异步
    details: Zig 风格无 async 关键字，任何 fn 都可 await；stackful 协程 + mio 事件驱动，慢处理器不阻塞事件循环。
  - title: 软件事务内存
    details: shared + transact 提供快照隔离的事务，原子提交、自动回滚、可嵌套。
  - title: 表达式导向
    details: if、match、loop 都是表达式，有返回值。代码更简洁，意图更清晰。
  - title: 模式匹配
    details: 强大的模式匹配支持字面量、解构、守卫、Or 模式，替代繁琐的条件分支。
  - title: 模块系统
    details: 内置 10 个标准库模块（io/json/crypto/tls/ffi 等），支持延迟导入和别名。
  - title: FFI
    details: 通过 libloading 加载动态库，支持 void/int/uint/float/str 五种 ABI 类型。
---
