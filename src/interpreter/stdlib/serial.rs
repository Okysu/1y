//! `serial` module — serial port I/O (RS-232, UART, USB-serial).
//!
//! Wraps the `serialport` crate. Supports common baud rates, data bits,
//! parity, and stop bits.
//!
//! Exports:
//!   - `open(port, baud) -> Opaque`           — open a serial port
//!   - `open_full(port, opts) -> Opaque`       — open with full options Map
//!   - `read(port, n) -> Str | Nil`            — read up to n bytes
//!   - `read_line(port) -> Str | Nil`          — read until newline
//!   - `write(port, data)`                     — write a string
//!   - `close(port)`                           — close the port
//!   - `list() -> Vec<Str>`                    — list available port names
//!
//! Options Map keys for `open_full`:
//!   - "baud": Int (default 9600)
//!   - "data_bits": Int 5/6/7/8 (default 8)
//!   - "parity": "none" | "odd" | "even" (default "none")
//!   - "stop_bits": 1 | 2 (default 1)
//!   - "flow_control": "none" | "software" | "hardware" (default "none")
//!   - "timeout_ms": Int (default 1000)

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, NativeResource, Value};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::rc::Rc;
use std::time::Duration;

pub fn build() -> ModuleRef {
    make_module(
        "serial",
        &[
            ("open", NativeFn { name: "open", func: bi_open }),
            ("open_full", NativeFn { name: "open_full", func: bi_open_full }),
            ("read", NativeFn { name: "read", func: bi_read }),
            ("read_line", NativeFn { name: "read_line", func: bi_read_line }),
            ("write", NativeFn { name: "write", func: bi_write }),
            ("close", NativeFn { name: "close", func: bi_close }),
            ("list", NativeFn { name: "list", func: bi_list }),
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

fn serial_at(args: &[Value], idx: usize, name: &str) -> Result<Rc<RefCell<Box<dyn serialport::SerialPort>>>, InterpreterError> {
    let r = opaque_at(args, idx, name)?;
    match &*r {
        NativeResource::Serial(s) => Ok(s.clone()),
        _ => Err(InterpreterError::TypeError {
            expected: "SerialPort", got: "opaque", op: name.into(), span: None,
        }),
    }
}

fn bi_open(args: &[Value]) -> Result<Value, InterpreterError> {
    let port = str_at(args, 0, "serial.open")?;
    let baud = match args.get(1) {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_u32(n).unwrap_or(9600),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "serial.open".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "serial.open".into(), span: None,
        }),
    };
    let s = serialport::new(&port, baud)
        .timeout(Duration::from_millis(1000))
        .open()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("serial.open: {}", e), span: None,
        })?;
    Ok(Value::Opaque(Rc::new(NativeResource::Serial(Rc::new(RefCell::new(s))))))
}

fn bi_open_full(args: &[Value]) -> Result<Value, InterpreterError> {
    let port = str_at(args, 0, "serial.open_full")?;
    let opts = match args.get(1) {
        Some(Value::Map(m)) => m,
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Map", got: v.type_name(), op: "serial.open_full".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "serial.open_full".into(), span: None,
        }),
    };

    let get_int = |key: &str, default: u32| -> u32 {
        opts.get(&Value::str(key))
            .and_then(|v| if let Value::Int(n) = v { num_traits::ToPrimitive::to_u32(n) } else { None })
            .unwrap_or(default)
    };
    let get_str = |key: &str, default: &str| -> String {
        opts.get(&Value::str(key))
            .and_then(|v| if let Value::Str(s) = v { Some((**s).clone()) } else { None })
            .unwrap_or_else(|| default.to_string())
    };

    let baud = get_int("baud", 9600);
    let data_bits = match get_int("data_bits", 8) {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    };
    let parity = match get_str("parity", "none").as_str() {
        "odd" => serialport::Parity::Odd,
        "even" => serialport::Parity::Even,
        _ => serialport::Parity::None,
    };
    let stop_bits = match get_int("stop_bits", 1) {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    };
    let flow_control = match get_str("flow_control", "none").as_str() {
        "software" => serialport::FlowControl::Software,
        "hardware" => serialport::FlowControl::Hardware,
        _ => serialport::FlowControl::None,
    };
    let timeout_ms = get_int("timeout_ms", 1000);

    let s = serialport::new(&port, baud)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow_control)
        .timeout(Duration::from_millis(timeout_ms as u64))
        .open()
        .map_err(|e| InterpreterError::RuntimeError {
            msg: format!("serial.open_full: {}", e), span: None,
        })?;
    Ok(Value::Opaque(Rc::new(NativeResource::Serial(Rc::new(RefCell::new(s))))))
}

fn bi_read(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = serial_at(args, 0, "serial.read")?;
    let n = match args.get(1) {
        Some(Value::Int(n)) => num_traits::ToPrimitive::to_usize(n).unwrap_or(0),
        Some(v) => return Err(InterpreterError::TypeError {
            expected: "Int", got: v.type_name(), op: "serial.read".into(), span: None,
        }),
        None => return Err(InterpreterError::ArityError {
            expected: 2, got: args.len(), callee: "serial.read".into(), span: None,
        }),
    };
    let mut buf = vec![0u8; n];
    let read = s.borrow_mut().read(&mut buf).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("serial.read: {}", e), span: None,
    })?;
    if read == 0 {
        return Ok(Value::Nil);
    }
    buf.truncate(read);
    Ok(Value::str(String::from_utf8_lossy(&buf).to_string()))
}

fn bi_read_line(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = serial_at(args, 0, "serial.read_line")?;
    // Clone the underlying serial port to wrap in a BufReader without
    // holding the borrow.
    let cloned = s.borrow().try_clone().map_err(|e| InterpreterError::RuntimeError {
        msg: format!("serial.read_line: {}", e), span: None,
    })?;
    let mut reader = BufReader::new(cloned);
    let mut line = String::new();
    let read = reader.read_line(&mut line).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("serial.read_line: {}", e), span: None,
    })?;
    if read == 0 {
        return Ok(Value::Nil);
    }
    if line.ends_with('\n') { line.pop(); }
    if line.ends_with('\r') { line.pop(); }
    Ok(Value::str(line))
}

fn bi_write(args: &[Value]) -> Result<Value, InterpreterError> {
    let s = serial_at(args, 0, "serial.write")?;
    let data = str_at(args, 1, "serial.write")?;
    s.borrow_mut().write_all(data.as_bytes()).map_err(|e| InterpreterError::RuntimeError {
        msg: format!("serial.write: {}", e), span: None,
    })?;
    Ok(Value::Nil)
}

fn bi_close(_args: &[Value]) -> Result<Value, InterpreterError> {
    // The serial port is closed when the last Rc reference is dropped.
    // We don't need to do anything explicit here.
    Ok(Value::Nil)
}

fn bi_list(_args: &[Value]) -> Result<Value, InterpreterError> {
    let ports = serialport::available_ports().map_err(|e| InterpreterError::RuntimeError {
        msg: format!("serial.list: {}", e), span: None,
    })?;
    let names: Vec<Value> = ports.into_iter().map(|p| Value::str(p.port_name)).collect();
    Ok(Value::vec(names))
}
