---
title: TLS HTTP Client
---

# TLS HTTP Client

The vast majority of modern network requests run over HTTPS, which means any language that wants to touch the real world must be able to complete a TLS handshake. 1y's `tls` module is built on `rustls` with the Mozilla root certificates baked in, and a single `connect` call performs certificate verification — what you get back is an encrypted stream you can read and write. This example writes a minimal working HTTPS client from scratch: issue a `GET` request, read the response, parse the status line, and handle network errors with `try`/`rescue`. Because 1y does not ship a high-level HTTP client, this example also helps you understand the smallest skeleton of the HTTP/1.1 protocol.

## Establishing a TLS Connection

It all starts with `tls.connect`:

```1y
import tls;

let host = "example.com";
let stream = tls.connect(host, 443);
println("connected to: " + tls.peer_addr(stream));
```

`tls.connect(host, 443)` performs the full TLS handshake, including domain verification and certificate-chain validation. **The handshake is eager** — if there is a problem with the certificate, `connect` raises immediately rather than waiting until the first read or write. This means that as soon as this line returns successfully, you hold a trusted, encrypted channel. `tls.peer_addr(stream)` returns the remote address, handy for debugging. Note that `stream` is an opaque resource handle; you can only manipulate it through functions in the `tls` module.

## Building the HTTP Request

An HTTP/1.1 request is, at its core, a piece of correctly formatted text. We assemble a `GET` request by hand:

```1y
fn build_get_request(host, path) -> Str {
    "GET {path} HTTP/1.1\r\n" +
    "Host: {host}\r\n" +
    "User-Agent: 1y-http-demo/0.1\r\n" +
    "Connection: close\r\n" +
    "\r\n"
}
```

Let us walk through this request text line by line:

- **The request line** `GET {path} HTTP/1.1\r\n` declares the method, path, and protocol version. The `{path}` here is string interpolation, replaced at runtime by the argument. `\r\n` is the line terminator mandated by HTTP (CRLF) — Windows-style newline — never write it as `\n`.
- **The `Host` header** is mandatory in HTTP/1.1, because one server may host several domains via virtual hosting and needs this to disambiguate.
- **The `User-Agent` header** identifies the client. Many servers reject requests that lack a `User-Agent`, so including one is a good habit.
- **`Connection: close`** tells the server to close the connection once the response is sent. This lets us simply read until EOF to obtain the full response body, instead of parsing `Content-Length`.
- **The blank line `\r\n`** marks the end of the headers; the server does not start processing until it sees it.

We use `+` to concatenate strings here purely for readability; you could equally well put the whole request inside a triple-quoted string. The interpolated `{path}` lets the function be reused for any path.

## Sending the Request and Reading the Response

With the request text in hand, write it to the stream and read the response in a loop:

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

A few key points:

- **`tls.write(stream, request)`** writes the string into the encrypted stream; the content is encrypted by the TLS layer before being sent. It returns `Nil` and not a byte count.
- **`tls.read(stream, 4096)`** reads up to 4096 bytes and returns a string. When the connection closes (we set `Connection: close`, so the server closes after sending the response), it returns `nil`.
- We use `loop { ... break response }` to read repeatedly and accumulate. `loop` is an infinite loop, and `break response` exits the loop and yields `response` as the value of the entire `loop` expression. This is the idiomatic "read until EOF" pattern in 1y.
- When `chunk` is `nil`, we `break response`, carrying the accumulated full response out of the loop.

## Parsing the Status Line

The first line of the response is the status line, shaped like `HTTP/1.1 200 OK`. We slice it out:

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

`split(response, "\r\n")` breaks the response into lines; `first` takes the first one; splitting that by spaces yields three pieces — the protocol version, the status code, and the reason phrase. `parts[1]` is the status code as a string, such as `"200"`. In production code you should check whether `code` starts with `2` to decide success, rather than assuming the request always went as hoped.

## Handling Errors with try/rescue

Networks are unreliable: DNS failures, expired certificates, reset connections — all of these surface as exceptions. 1y catches them with `try`/`rescue`:

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

`try { ... } rescue as e { ... }` catches any exception and binds the exception value to `e`. The `rescue` branch is itself an expression, and its value becomes the return value of the whole `try` — so on failure `fetch` returns a string beginning with `[error]` instead of crashing the program. This "exception values rather than error codes" style lets error handling share the same expression machinery as normal logic. In production, you can also use a patterned `rescue Pattern as e` to catch only specific exceptions and let the rest keep propagating.

## Recap

In a few dozen lines we have a working HTTPS client. The value of this example is not "yet another HTTP library," but the way it shows 1y handing low-level capabilities (TLS, sockets, strings) directly to the programmer: there is no implicit runtime magic — the request is text you assemble yourself, the response is bytes you read yourself, and errors are exceptions you handle yourself. When you later reach for a higher-level library, understanding this underlying model will give you a firmer grasp of network programming. And do not forget to clean up: call `tls.close(stream)` to close the stream. In a short script the program exit will release it anyway, but in a long-running service closing explicitly is a good habit.
