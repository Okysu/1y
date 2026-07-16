---
title: yin Web Framework
---

# yin Web Framework

`yin` is a Gin-inspired web framework, self-hosted in pure 1y, built on `lib.http`. It demonstrates that 1y's language features (shared cells, persistent collections, colorless async, actors) are sufficient to build a real web framework.

## Complete Example

```1y
import lib.yin as yin;

let app = yin.new();

// Middleware
yin.use(app, fn(ctx, next) {
    let c = ctx;
    let req = c["req"];
    println(req["method"] + " " + req["path"]);
    next()
});

// Basic routes
yin.get(app, "/", fn(ctx) {
    yin.html(ctx, 200, "<h1>Welcome to yin</h1>")
});

yin.get(app, "/ping", fn(ctx) {
    yin.json(ctx, 200, { "message": "pong" })
});

// Param route
yin.get(app, "/users/:id", fn(ctx) {
    let id = yin.param(ctx, "id");
    yin.json(ctx, 200, { "user_id": id })
});

// Route group
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

// Nested group
let v1 = yin.group(app, "/api/v1");
yin.get(v1, "/status", fn(ctx) {
    yin.json(ctx, 200, { "version": "v1", "status": "ok" })
});

// Start server
yin.run(app, "127.0.0.1:8080")
```

## Design

### App is a shared cell

App is a `shared` cell holding a Map with nested shared cells (routes/param_routes/middlewares), so route groups share the parent's route table.

### Context is a shared cell

Context is also a `shared` cell; `yin.json(ctx, ...)` mutates it in place, so handlers don't need to return the context.

### Groups share route tables

```1y
let api = yin.group(app, "/api");
// Routes registered on api write through to app's route table
yin.get(api, "/users", handler);
// Equivalent to yin.get(app, "/api/users", handler)
```

## API Reference

| Function | Description |
|----------|-------------|
| `yin.new()` | Create a new app |
| `yin.get/post/put/delete(app, path, handler)` | Register a route |
| `yin.use(app, middleware)` | Register middleware |
| `yin.group(app, prefix)` | Create a route group |
| `yin.handle(app, req)` | Dispatch a request (for testing) |
| `yin.run(app, addr)` | Start the server |
| `yin.param(ctx, name)` | Get a path parameter |
| `yin.json/html/text(ctx, status, data)` | Response helpers |
