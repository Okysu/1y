---
layout: home

hero:
  name: "1y"
  text: "一门流式并发函数式编程语言"
  tagline: 持久化数据结构 · 模式匹配 · Actor 并发 · 软件事务内存 · 任意精度数值
  actions:
    - theme: brand
      text: 中文文档
      link: /zh/syntax/getting-started
    - theme: alt
      text: English Docs
      link: /en/syntax/getting-started

features:
  - title: 数值统一
    details: 整数和小数都是任意精度，算术自动提升，不再有溢出和精度丢失的烦恼。
  - title: 持久化集合
    details: Vec、Map、Set 基于结构共享，"修改"集合返回新版本，旧版本仍然可用。
  - title: Actor 并发
    details: 基于 Actor 模型的消息传递并发，每个 Actor 拥有隔离状态，无数据竞争。
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
