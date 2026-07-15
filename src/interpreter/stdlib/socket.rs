//! `socket` module — TCP networking (synchronous, blocking).
//!
//! Uses `std::net` (blocking I/O). For concurrent servers, wrap socket
//! operations in an actor — the single-threaded event loop naturally
//! multiplexes actors over blocking I/O when combined with `process.sleep_ms`
//! polling or non-blocking mode.
//!
//! Exports:
//!   - `listen(addr) -> Opaque`         — create a TcpListener (binds to addr)
//!   - `accept(listener) -> Opaque`     — accept a connection, returns TcpStream
//!   - `connect(addr) -> Opaque`        — connect to a remote, returns TcpStream
//!   - `read(stream, n) -> Str | Nil`   — read up to n bytes (Nil on EOF)
//!   - `read_async(stream, n) -> Task`  — async read, returns Task<Str|Nil>
//!   - `read_line(stream) -> Str | Nil` — read until newline (Nil on EOF)
//!   - `write(stream, data)`            — write a string to the stream
//!   - `close(resource)`                — close a listener or stream
//!   - `set_nonblocking(resource, b)`   — set non-blocking mode
//!   - `peer_addr(stream) -> Str`       — remote address as "ip:port"

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, NativeResource, Value};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;

pub fn build() -> ModuleRef {
    make_module(
        "socket",
        &[
            ("listen", NativeFn { name: "listen", func: bi_listen }),
            ("accept", NativeFn { name: "accept", func: bi_accept }),
            ("connect", NativeFn { name: "connect", func: bi_connect }),
            ("read", NativeFn { name: "read", func: bi_read }),
            ("read_async", NativeFn { name: "read_async", func: bi_read_async }),
            ("read_line", NativeFn { name: "read_line", func: bi_read_line }),
            ("write", NativeFn { name: "write", func: bi_write }),
            ("close", NativeFn { name: "close", func: bi_close }),
            ("set_nonblocking", NativeFn { name: "set_nonblocking", func: bi_set_nonblocking }),
            ("peer_addr", NativeFn { name: "peer_addr", func: bi_peer_addr }),
        ],
    )
}

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

fn opaque_at(args: &[Value], idx: usize, name: &str) -> Result<Rc<NativeResource>, InterpreterError> {
    match args.get(idx) {
        Some(Value::Opaque(r)) => Ok(r.clone()),
        Some(v) => Err(InterpreterError::TypeError {
            expected: "Opaque", got: v.type_name(), op: name.into(), span: None,
        }),
        None => Err(InterpreterError::ArityError {
            expected: idx + 1, got: args.len(), callee: name.into(), span: None,
        }),
    }
}

fn bi_listen(args: &[Value]) -> Result<Value, InterpreterError> {
    let addr = str_at(args, 0, "socket.listen")?;
    let listener = TcpListener::bind(&addr).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("socket.listen: {}", e), span: None,
    })?;
    Ok(Value::Opaque(Rc::new(NativeResource::TcpListener(Rc::new(RefCell::new(listener))))))
}

fn bi_accept(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.accept")?;
    match &*r {
        NativeResource::TcpListener(l) => {
            match l.borrow().accept() {
                Ok((stream, _addr)) => {
                    Ok(Value::Opaque(Rc::new(NativeResource::TcpStream(Rc::new(RefCell::new(stream))))))
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Non-blocking mode: no pending connection — return Nil.
                    Ok(Value::Nil)
                }
                Err(e) => Err(InterpreterError::RuntimeError {
                    msg: format!("socket.accept: {}", e), span: None,
                }),
            }
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpListener", got: "opaque", op: "socket.accept".into(), span: None,
        }),
    }
}

fn bi_connect(args: &[Value]) -> Result<Value, InterpreterError> {
    let addr = str_at(args, 0, "socket.connect")?;
    let stream = TcpStream::connect(&addr).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("socket.connect: {}", e), span: None,
    })?;
    Ok(Value::Opaque(Rc::new(NativeResource::TcpStream(Rc::new(RefCell::new(stream))))))
}

fn bi_read(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.read")?;
    let n = match args.get(1) {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_usize(n).unwrap_or(0),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "socket.read".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "socket.read".into(), span: None,
        }),
    };
    match &*r {
        NativeResource::TcpStream(s) => {
            use std::io::Read;
            let mut buf = vec![0u8; n];
            let read = match s.borrow_mut().read(&mut buf) {
                Ok(0) => return Ok(Value::Nil), // EOF
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Non-blocking mode: no data available yet — return Nil.
                    return Ok(Value::Nil);
                }
                Err(e) => return Err(InterpreterError::RuntimeError {
                    msg: format!("socket.read: {}", e), span: None,
                }),
            };
            buf.truncate(read);
            Ok(Value::str(String::from_utf8_lossy(&buf).to_string()))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpStream", got: "opaque", op: "socket.read".into(), span: None,
        }),
    }
}

/// `socket.read_async(stream, n) -> Task<Str|Nil>` — async non-blocking read.
/// Returns a Task that polls the stream (which must be in non-blocking mode).
/// When data is available, completes with `Str`. On EOF, completes with `Nil`.
///
/// The stream is registered with the scheduler's mio::Poll for event-driven
/// I/O multiplexing — the Task is only polled when mio reports the stream
/// as readable, avoiding O(n) polling of all parked Tasks.
fn bi_read_async(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.read_async")?;
    let n = match args.get(1) {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_usize(n).unwrap_or(0),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "socket.read_async".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "socket.read_async".into(), span: None,
        }),
    };
    match &*r {
        NativeResource::TcpStream(s) => {
            let s_for_closure = s.clone();
            let task_state = crate::value::TaskState::Pending(Box::new(move || {
                use std::io::Read;
                let mut buf = vec![0u8; n];
                match s_for_closure.borrow_mut().read(&mut buf) {
                    Ok(0) => crate::value::TaskPoll::Ready(Value::Nil),
                    Ok(read) => {
                        buf.truncate(read);
                        crate::value::TaskPoll::Ready(Value::str(
                            String::from_utf8_lossy(&buf).to_string(),
                        ))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        crate::value::TaskPoll::Pending
                    }
                    Err(_) => crate::value::TaskPoll::Ready(Value::Nil),
                }
            }));
            let task_ref = std::rc::Rc::new(std::cell::RefCell::new(task_state));
            // Register the stream with the scheduler's mio::Poll for
            // event-driven readiness notification. This makes the scheduler
            // only poll this Task when the stream is actually readable.
            // `s` is still alive here (only `s_for_closure` was moved).
            crate::runtime::scheduler::register_readable(&s.borrow(), &task_ref);
            Ok(Value::Task(task_ref))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpStream", got: "opaque", op: "socket.read_async".into(), span: None,
        }),
    }
}

fn bi_read_line(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.read_line")?;
    match &*r {
        NativeResource::TcpStream(s) => {
            // We need a BufReader, but creating one borrows the stream. Since
            // we hold the stream in a RefCell, create a temporary clone via
            // try_clone to avoid borrow issues.
            let cloned = s.borrow().try_clone().map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.read_line: {}", e), span: None,
            })?;
            let mut reader = BufReader::new(cloned);
            let mut line = String::new();
            let read = reader.read_line(&mut line).map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.read_line: {}", e), span: None,
            })?;
            if read == 0 {
                return Ok(Value::Nil);
            }
            // Strip trailing newline.
            if line.ends_with('\n') { line.pop(); }
            if line.ends_with('\r') { line.pop(); }
            Ok(Value::str(line))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpStream", got: "opaque", op: "socket.read_line".into(), span: None,
        }),
    }
}

fn bi_write(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.write")?;
    let data = str_at(args, 1, "socket.write")?;
    match &*r {
        NativeResource::TcpStream(s) => {
            s.borrow_mut().write_all(data.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.write: {}", e), span: None,
            })?;
            Ok(Value::Nil)
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpStream", got: "opaque", op: "socket.write".into(), span: None,
        }),
    }
}

fn bi_close(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.close")?;
    match &*r {
        NativeResource::TcpListener(l) => {
            let _ = l;
            Ok(Value::Nil)
        }
        NativeResource::TcpStream(s) => {
            let _ = s.borrow().shutdown(std::net::Shutdown::Both);
            Ok(Value::Nil)
        }
        NativeResource::Serial(_) => Ok(Value::Nil),
        NativeResource::TlsStream(_) => Ok(Value::Nil),
        NativeResource::SharedLib(_) => Ok(Value::Nil),
    }
}

fn bi_set_nonblocking(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.set_nonblocking")?;
    let on = match args.get(1) {
        Some(Value::Bool(b)) => *b,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Bool", got: v.type_name(), op: "socket.set_nonblocking".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "socket.set_nonblocking".into(), span: None,
        }),
    };
    match &*r {
        NativeResource::TcpListener(l) => {
            l.borrow().set_nonblocking(on).map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.set_nonblocking: {}", e), span: None,
            })?;
        }
        NativeResource::TcpStream(s) => {
            s.borrow().set_nonblocking(on).map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.set_nonblocking: {}", e), span: None,
            })?;
        }
        NativeResource::Serial(_) => {}
        NativeResource::TlsStream(_) => {}
        NativeResource::SharedLib(_) => {}
    }
    Ok(Value::Nil)
}

fn bi_peer_addr(args: &[Value]) -> Result<Value, InterpreterError> {
    let r = opaque_at(args, 0, "socket.peer_addr")?;
    match &*r {
        NativeResource::TcpStream(s) => {
            let addr = s.borrow().peer_addr().map_err(|e| InterpreterError::RuntimeError {
                msg: format!("socket.peer_addr: {}", e), span: None,
            })?;
            Ok(Value::str(addr.to_string()))
        }
        _ => Err(InterpreterError::TypeError {
            expected: "TcpStream", got: "opaque", op: "socket.peer_addr".into(), span: None,
        }),
    }
}
