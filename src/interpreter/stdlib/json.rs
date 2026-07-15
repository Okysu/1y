//! `json` module — JSON parsing and serialization.
//!
//! A self-contained JSON parser/serializer (no external dependency) that
//! preserves arbitrary-precision integers by mapping them to `Value::Int`.
//!
//! Exports:
//!   - `parse(str) -> Value`        — parse a JSON string into a 1y value
//!   - `stringify(value) -> Str`    — serialize a 1y value to a JSON string
//!   - `pretty(value, indent) -> Str` — serialize with indentation
//!
//! Type mapping:
//!   JSON null  ↔ Nil
//!   JSON bool  ↔ Bool
//!   JSON number (integer) ↔ Int
//!   JSON number (fractional) ↔ Decimal
//!   JSON string ↔ Str
//!   JSON array ↔ Vec
//!   JSON object ↔ Map (keys are Str)

use crate::interpreter::error::InterpreterError;
use crate::interpreter::stdlib::make_module;
use crate::value::{ModuleRef, NativeFn, Value};
use bigdecimal::BigDecimal;
use num_bigint::BigInt;

pub fn build() -> ModuleRef {
    make_module(
        "json",
        &[
            ("parse", NativeFn { name: "parse", func: bi_parse }),
            ("stringify", NativeFn { name: "stringify", func: bi_stringify }),
            ("pretty", NativeFn { name: "pretty", func: bi_pretty }),
        ],
    )
}

fn one_arg(args: &[Value], name: &str) -> Result<Value, InterpreterError> {
    args.first().cloned().ok_or_else(|| InterpreterError::ArityError {
        expected: 1,
        got: args.len(),
        callee: name.into(),
        span: None,
    })
}

fn bi_parse(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "json.parse")?;
    let s = match &v {
        Value::Str(s) => s.as_str(),
        _ => return Err(InterpreterError::TypeError {
            expected: "Str",
            got: v.type_name(),
            op: "json.parse".into(),
            span: None,
        }),
    };
    let mut parser = JsonParser::new(s);
    parser.skip_ws();
    let result = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos < parser.bytes.len() {
        return Err(InterpreterError::RuntimeError {
            msg: format!("json.parse: trailing characters at position {}", parser.pos),
            span: None,
        });
    }
    Ok(result)
}

fn bi_stringify(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "json.stringify")?;
    let mut out = String::new();
    stringify_into(&mut out, &v, None, 0)?;
    Ok(Value::str(out))
}

fn bi_pretty(args: &[Value]) -> Result<Value, InterpreterError> {
    let v = one_arg(args, "json.pretty")?;
    let indent = match args.get(1) {
        Some(Value::Int(n)) => n.to_string().parse::<usize>().unwrap_or(2),
        _ => 2,
    };
    let mut out = String::new();
    stringify_into(&mut out, &v, Some(indent), 0)?;
    Ok(Value::str(out))
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

fn stringify_into(
    out: &mut String,
    v: &Value,
    indent: Option<usize>,
    depth: usize,
) -> Result<(), InterpreterError> {
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Decimal(d) => out.push_str(&d.to_string()),
        Value::Str(s) => {
            out.push('"');
            escape_str_into(out, s);
            out.push('"');
        }
        Value::Vec(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push('[');
            if let Some(n) = indent {
                out.push('\n');
                push_indent(out, n, depth + 1);
            }
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_some() {
                        out.push('\n');
                        push_indent(out, indent.unwrap(), depth + 1);
                    }
                }
                stringify_into(out, item, indent, depth + 1)?;
            }
            if indent.is_some() {
                out.push('\n');
                push_indent(out, indent.unwrap(), depth);
            }
            out.push(']');
        }
        Value::Map(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push('{');
            if indent.is_some() {
                out.push('\n');
                push_indent(out, indent.unwrap(), depth + 1);
            }
            let mut entries: Vec<_> = m.iter().collect();
            // Sort keys for deterministic output (JSON objects are unordered,
            // but deterministic serialization helps testing).
            entries.sort_by(|a, b| format!("{}", a.0).cmp(&format!("{}", b.0)));
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_some() {
                        out.push('\n');
                        push_indent(out, indent.unwrap(), depth + 1);
                    }
                }
                let key_str = match k {
                    Value::Str(s) => (**s).clone(),
                    _ => format!("{}", k),
                };
                out.push('"');
                escape_str_into(out, &key_str);
                out.push('"');
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                stringify_into(out, v, indent, depth + 1)?;
            }
            if indent.is_some() {
                out.push('\n');
                push_indent(out, indent.unwrap(), depth);
            }
            out.push('}');
        }
        _ => return Err(InterpreterError::RuntimeError {
            msg: format!("json.stringify: cannot serialize {}", v.type_name()),
            span: None,
        }),
    }
    Ok(())
}

fn push_indent(out: &mut String, n: usize, depth: usize) {
    for _ in 0..(n * depth) {
        out.push(' ');
    }
}

fn escape_str_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        JsonParser { bytes: s.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Result<Value, InterpreterError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Value::str),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(InterpreterError::RuntimeError {
                msg: format!("json.parse: unexpected character '{}' at position {}", c as char, self.pos),
                span: None,
            }),
            None => Err(InterpreterError::RuntimeError {
                msg: "json.parse: unexpected end of input".into(),
                span: None,
            }),
        }
    }

    fn parse_object(&mut self) -> Result<Value, InterpreterError> {
        self.pos += 1; // consume '{'
        let mut entries: Vec<(Value, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::map(entries));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(InterpreterError::RuntimeError {
                    msg: format!("json.parse: expected ':' at position {}", self.pos),
                    span: None,
                });
            }
            self.pos += 1;
            let val = self.parse_value()?;
            entries.push((Value::str(key), val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b'}') => { self.pos += 1; break; }
                _ => return Err(InterpreterError::RuntimeError {
                    msg: format!("json.parse: expected ',' or '}}' at position {}", self.pos),
                    span: None,
                }),
            }
        }
        Ok(Value::map(entries))
    }

    fn parse_array(&mut self) -> Result<Value, InterpreterError> {
        self.pos += 1; // consume '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::vec(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b']') => { self.pos += 1; break; }
                _ => return Err(InterpreterError::RuntimeError {
                    msg: format!("json.parse: expected ',' or ']' at position {}", self.pos),
                    span: None,
                }),
            }
        }
        Ok(Value::vec(items))
    }

    fn parse_string(&mut self) -> Result<String, InterpreterError> {
        if self.peek() != Some(b'"') {
            return Err(InterpreterError::RuntimeError {
                msg: format!("json.parse: expected '\"' at position {}", self.pos),
                span: None,
            });
        }
        self.pos += 1;
        let mut s = String::new();
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' => { self.pos += 1; return Ok(s); }
                b'\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'b') => s.push('\u{08}'),
                        Some(b'f') => s.push('\u{0c}'),
                        Some(b'u') => {
                            self.pos += 1;
                            if self.pos + 4 > self.bytes.len() {
                                return Err(InterpreterError::RuntimeError {
                                    msg: "json.parse: incomplete \\u escape".into(),
                                    span: None,
                                });
                            }
                            let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
                                .map_err(|_| InterpreterError::RuntimeError {
                                    msg: "json.parse: invalid \\u escape".into(),
                                    span: None,
                                })?;
                            let code = u32::from_str_radix(hex, 16).map_err(|_| InterpreterError::RuntimeError {
                                msg: format!("json.parse: invalid \\u escape: {}", hex),
                                span: None,
                            })?;
                            if let Some(c) = char::from_u32(code) {
                                s.push(c);
                            }
                            self.pos += 3;
                        }
                        _ => return Err(InterpreterError::RuntimeError {
                            msg: format!("json.parse: invalid escape at position {}", self.pos),
                            span: None,
                        }),
                    }
                    self.pos += 1;
                }
                c => {
                    // UTF-8: collect raw bytes, decode later.
                    s.push(c as char);
                    self.pos += 1;
                }
            }
        }
        Err(InterpreterError::RuntimeError {
            msg: "json.parse: unterminated string".into(),
            span: None,
        })
    }

    fn parse_bool(&mut self) -> Result<Value, InterpreterError> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(Value::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(Value::Bool(false))
        } else {
            Err(InterpreterError::RuntimeError {
                msg: format!("json.parse: invalid literal at position {}", self.pos),
                span: None,
            })
        }
    }

    fn parse_null(&mut self) -> Result<Value, InterpreterError> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(Value::Nil)
        } else {
            Err(InterpreterError::RuntimeError {
                msg: format!("json.parse: invalid literal at position {}", self.pos),
                span: None,
            })
        }
    }

    fn parse_number(&mut self) -> Result<Value, InterpreterError> {
        let start = self.pos;
        if self.peek() == Some(b'-') { self.pos += 1; }
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) { self.pos += 1; }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let num_str = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| InterpreterError::RuntimeError {
            msg: "json.parse: invalid number".into(),
            span: None,
        })?;
        if is_float {
            num_str.parse::<BigDecimal>().map(Value::Decimal).map_err(|_| InterpreterError::RuntimeError {
                msg: format!("json.parse: invalid number: {}", num_str),
                span: None,
            })
        } else {
            num_str.parse::<BigInt>().map(Value::Int).map_err(|_| InterpreterError::RuntimeError {
                msg: format!("json.parse: invalid number: {}", num_str),
                span: None,
            })
        }
    }
}
