//! `tls` module — TLS/SSL client connections (rustls + webpki-roots).
//!
//! Uses `rustls` 0.23 with the `ring` crypto provider and Mozilla's root CA
//! bundle (`webpki-roots`) for verification. Only client-side connections are
//! supported; for concurrent TLS servers, wrap operations in an actor.
//!
//! Exports:
//!   - `connect(host, port) -> Opaque`    — open a TLS client connection
//!   - `read(stream, n) -> Str | Nil`     — read up to n bytes (Nil on EOF)
//!   - `read_line(stream) -> Str | Nil`   — read until newline (Nil on EOF)
//!   - `write(stream, data)`              — write a string to the stream
//!   - `close(stream)`                    — close the TLS stream
//!   - `peer_addr(stream) -> Str`         — remote address as "ip:port"

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, NativeResource, Value};
use rustls::{ClientConfig, ClientConnection};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::sync::OnceLock;

pub fn build() -> ModuleRef {
    make_module(
        "tls",
        &[
            ("connect", NativeFn { name: "connect", func: bi_connect }),
            ("read", NativeFn { name: "read", func: bi_read }),
            ("read_line", NativeFn { name: "read_line", func: bi_read_line }),
            ("write", NativeFn { name: "write", func: bi_write }),
            ("close", NativeFn { name: "close", func: bi_close }),
            ("peer_addr", NativeFn { name: "peer_addr", func: bi_peer_addr }),
        ],
    )
}

// Build the ClientConfig once and reuse it. The ring provider is installed on
// first use. Using OnceLock keeps the builder lazy and thread-safe-ish (the
// interpreter is single-threaded, but OnceLock avoids rebuilding per call).
static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

use std::sync::Arc;

fn shared_config() -> Result<Arc<ClientConfig>, InterpreterError> {
    if let Some(cfg) = CONFIG.get() {
        return Ok(cfg.clone());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("tls: protocol versions: {}", e),
            span: None,
        })?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let arc = Arc::new(cfg);
    // Race is fine — single-threaded interpreter; fall back to existing.
    let _ = CONFIG.set(arc.clone());
    Ok(arc)
}

// --- helpers ---

fn str_at(args: &[Value], idx: usize, name: &str) -> Result<String, InterpreterError> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok((**s).clone()),
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Str", got: v.type_name(), op: name.into(), span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    }
}

fn tls_at(args: &[Value], idx: usize, name: &str) -> Result<Rc<RefCell<rustls::StreamOwned<ClientConnection, TcpStream>>>, InterpreterError> {
    let r = match args.get(idx) {
        Some(Value::Opaque(r)) => r.clone(),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Opaque", got: v.type_name(), op: name.into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    };
    match &*r {
        NativeResource::TlsStream(s) => Ok(s.clone()),
        _ => Err(InterpreterError::TypeError {
            expected: "TlsStream", got: "opaque", op: name.into(), span: None,
        }),
    }
}

// --- functions ---

fn bi_connect(args: &[Value]) -> Result<Value, InterpreterError> {
    let host = str_at(args, 0, "tls.connect")?;
    let port = match args.get(1) {
        Some(Value::Int(n)) => n,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "tls.connect".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "tls.connect".into(), span: None,
        }),
    };
    let port_u16 = {
        use num_traits::ToPrimitive;
        port.to_u16().ok_or_else(|| InterpreterError::RuntimeError {
            msg: "tls.connect: port out of range".into(),
            span: None,
        })?
    };

    let cfg = shared_config()?;
    let server_name = rustls::pki_types::ServerName::try_from(host.clone())
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("tls.connect: invalid host {}: {}", host, e),
            span: None,
        })?;
    let conn = ClientConnection::new(cfg, server_name)
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("tls.connect: {}", e),
            span: None,
        })?;

    let addr = format!("{}:{}", host, port_u16);
    let sock = TcpStream::connect(&addr).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("tls.connect: {}", e),
        span: None,
    })?;
    let mut stream = rustls::StreamOwned::new(conn, sock);
    // Force the TLS handshake eagerly so certificate/protocol errors surface
    // at connect time rather than on the first read/write.
    stream.flush().map_err(|e| InterpreterError::RuntimeError {
        msg: format!("tls.connect: handshake: {}", e),
        span: None,
    })?;
    Ok(Value::Opaque(Rc::new(NativeResource::TlsStream(Rc::new(RefCell::new(stream))))))
}

fn bi_read(args: &[Value]) -> Result<Value, InterpreterError> {
    let stream = tls_at(args, 0, "tls.read")?;
    let n = match args.get(1) {
        Some(Value::Int(n)) => n,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "tls.read".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "tls.read".into(), span: None,
        }),
    };
    use num_traits::ToPrimitive;
    let len = n.to_usize().ok_or_else(|| InterpreterError::RuntimeError {
        msg: "tls.read: n out of range".into(),
        span: None,
    })?;
    let mut buf = vec![0u8; len];
    let read = {
        let mut s = stream.borrow_mut();
        match s.read(&mut buf) {
            Ok(0) => return Ok(Value::Nil),
            Ok(k) => k,
            Err(e) => return Err(InterpreterError::RuntimeError {
                msg: format!("tls.read: {}", e),
                span: None,
            }),
        }
    };
    buf.truncate(read);
    Ok(Value::str(String::from_utf8_lossy(&buf)))
}

fn bi_read_line(args: &[Value]) -> Result<Value, InterpreterError> {
    let stream = tls_at(args, 0, "tls.read_line")?;
    // Wrap in BufReader for line reads. We borrow the stream mutably; leftover
    // buffered data is lost when the reader drops (acceptable for line protocols).
    let mut s = stream.borrow_mut();
    let mut reader = BufReader::new(&mut *s);
    let mut buf = String::new();
    let read = match reader.read_line(&mut buf) {
        Ok(0) => return Ok(Value::Nil),
        Ok(k) => k,
        Err(e) => return Err(InterpreterError::RuntimeError {
            msg: format!("tls.read_line: {}", e),
            span: None,
        }),
    };
    let _ = read;
    // Strip trailing newline to match socket.read_line semantics.
    let trimmed = buf.trim_end_matches(['\r', '\n']);
    Ok(Value::str(trimmed))
}

fn bi_write(args: &[Value]) -> Result<Value, InterpreterError> {
    let stream = tls_at(args, 0, "tls.write")?;
    let data = str_at(args, 1, "tls.write")?;
    {
        let mut s = stream.borrow_mut();
        s.write_all(data.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
            msg: format!("tls.write: {}", e),
            span: None,
        })?;
        s.flush().map_err(|e| InterpreterError::RuntimeError {
            msg: format!("tls.write: flush: {}", e),
            span: None,
        })?;
    }
    Ok(Value::Nil)
}

fn bi_close(args: &[Value]) -> Result<Value, InterpreterError> {
    let stream = tls_at(args, 0, "tls.close")?;
    let mut s = stream.borrow_mut();
    let _ = s.flush();
    let _ = s.sock.shutdown(std::net::Shutdown::Both);
    Ok(Value::Nil)
}

fn bi_peer_addr(args: &[Value]) -> Result<Value, InterpreterError> {
    let stream = tls_at(args, 0, "tls.peer_addr")?;
    let s = stream.borrow();
    let addr = s.sock.peer_addr().map_err(|e| InterpreterError::RuntimeError {
        msg: format!("tls.peer_addr: {}", e),
        span: None,
    })?;
    Ok(Value::str(addr.to_string()))
}
