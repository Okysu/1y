//! `1y` command-line entry point.
//!
//! Subcommands:
//!   - `1y run <file>`     parse and evaluate the file
//!   - `1y parse <file>`   parse the file and print the AST
//!   - `1y tokens <file>`  print the token stream
//!   - `1y ast -`          read source from stdin, print AST

use std::io::Read;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage(&args[0]);
        std::process::exit(2);
    }
    match args[1].as_str() {
        "run" => {
            let src = read_source(args.get(2));
            let entry_dir = args.get(2).and_then(|p| {
                if p == "-" {
                    None
                } else {
                    PathBuf::from(p).parent().map(|p| p.to_path_buf())
                }
            });
            // Tree-walking recursion maps to native recursion; run on a thread
            // with a large stack so deep `1y` recursion doesn't overflow.
            // `Value`/`InterpreterError` hold `Rc` (`!Send`), so the error is
            // stringified inside the worker thread before being returned.
            let result = std::thread::Builder::new()
                .stack_size(256 * 1024 * 1024)
                .spawn(move || {
                    let mut interp = onely::Interpreter::new();
                    if let Some(dir) = entry_dir {
                        interp.set_entry_dir(dir);
                    }
                    match interp.run(&src) {
                        Ok(()) => Ok(()),
                        Err(e) => Err(e.render(&src)),
                    }
                })
                .expect("spawn interpreter thread")
                .join()
                .expect("interpreter thread panicked");
            if let Err(e) = result {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        "parse" => {
            let src = read_source(args.get(2));
            let out = onely::parse(&src);
            print!("{}", onely::printer::print_program(&out.program));
            if !out.errors.is_empty() {
                eprintln!(
                    "{}",
                    out.errors
                        .iter()
                        .map(|e| e.render(&src))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                std::process::exit(1);
            }
        }
        "tokens" => {
            let src = read_source(args.get(2));
            let lex = onely::tokenize(&src);
            for t in &lex.tokens {
                println!("{:>6}:{:<3} {:?}", t.span.start.line, t.span.start.col, t.kind);
            }
            if !lex.errors.is_empty() {
                eprintln!("{}", lex.errors.iter().map(|e| e.render(&src)).collect::<Vec<_>>().join("\n"));
                std::process::exit(1);
            }
        }
        "help" | "--help" | "-h" => usage(&args[0]),
        other => {
            eprintln!("unknown subcommand: `{}`", other);
            usage(&args[0]);
            std::process::exit(2);
        }
    }
}

fn read_source(arg: Option<&String>) -> String {
    match arg {
        Some(p) if p == "-" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).expect("read stdin");
            s
        }
        Some(p) => {
            let path = PathBuf::from(p);
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("failed to read `{}`: {}", p, e);
                std::process::exit(2);
            })
        }
        None => {
            eprintln!("missing file path");
            std::process::exit(2);
        }
    }
}

fn usage(prog: &str) {
    eprintln!("1y — interpreter for the 1y language");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    {} run <file|->      parse and evaluate the source", prog);
    eprintln!("    {} parse <file|->    parse source, print the AST", prog);
    eprintln!("    {} tokens <file|->   print the token stream", prog);
    eprintln!("    {} help              show this message", prog);
}
