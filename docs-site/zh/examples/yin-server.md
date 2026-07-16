---
title: yin Web 框架
---

# yin Web 框架

`yin` 是受 Gin 启发的 Web 框架，用纯 1y 自举实现，构建在 `lib.http` 之上。它展示了 1y 的语言特性（shared cell、持久化集合、无色异步、actor）足以构建真正的 Web 框架。

## 完整示例

```1y
import lib.yin as yin;

let app = yin.new();

// 中间件
yin.use(app, fn(ctx, next) {
    let c = ctx;
    let req = c["req"];
    println(req["method"] + " " + req["path"]);
    next()
});

// 基础路由
yin.get(app, "/", fn(ctx) {
    yin.html(ctx, 200, "<h1>Welcome to yin</h1>")
});

yin.get(app, "/ping", fn(ctx) {
    yin.json(ctx, 200, { "message": "pong" })
});

// 参数路由
yin.get(app, "/users/:id", fn(ctx) {
    let id = yin.param(ctx, "id");
    yin.json(ctx, 200, { "user_id": id })
});

// 路由组
let api = yin.group(app, "/api");

yin.get(api, "/users", fn(ctx) {
    yin.json(ctx, 200, [
        { "id": 1, "name": "alice" },
        { "id": 2, "name": "bob" }
    ])
});

yin.post(api, "/users", fn(ctx) {
    let body = yin.body(ctx);
    yin.json(ctx, 201, { "created": true, "received": body })
});

// 嵌套路由组
let v1 = yin.group(app, "/api/v1");
yin.get(v1, "/status", fn(ctx) {
    yin.json(ctx, 200, { "version": "v1", "status": "ok" })
});

// 启动服务器
yin.run(app, "127.0.0.1:8080")
```

## 设计要点

### App 是 shared cell

App 是一个 `shared` cell 持有 Map，Map 内嵌套 shared cell（routes/param_routes/middlewares），使路由组能共享父 app 的路由表。

### Context 是 shared cell

Context 也是 `shared` cell；`yin.json(ctx, ...)` 原地修改它，处理器不需要返回 context。

### 路由组共享路由表

```1y
let api = yin.group(app, "/api");
// 在 api 上注册的路由会写入 app 的路由表
yin.get(api, "/users", handler);
// 等同于 yin.get(app, "/api/users", handler)
```

## API 一览

| 函数 | 说明 |
|------|------|
| `yin.new()` | 创建新 app |
| `yin.get/post/put/delete(app, path, handler)` | 注册路由 |
| `yin.use(app, middleware)` | 注册中间件 |
| `yin.group(app, prefix)` | 创建路由组 |
| `yin.handle(app, req)` | 处理请求（测试用） |
| `yin.run(app, addr)` | 启动服务器 |
| `yin.param(ctx, name)` | 获取路径参数 |
| `yin.json/html/text(ctx, status, data)` | 响应辅助函数 |
