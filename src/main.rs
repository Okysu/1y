//! `1y` command-line entry point.
//!
//! Subcommands:
//!   - `1y <file>`        parse and evaluate via the bytecode VM (default)
//!   - `1y vm <file>`     same as default (explicit)
//!   - `1y run <file>`    parse and evaluate via the tree-walking interpreter
//!   - `1y selfvm <file>` run via the self-hosted 1y VM (bootstrap/vm.1y)
//!   - `1y parse <file>`  parse the file and print the AST
//!   - `1y tokens <file>` print the token stream

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
            // Tree-walking interpreter. Recurses on the native call stack,
            // so it overflows on deep recursion (e.g. fib_memo(100)). Kept
            // as a fallback for comparison / debugging.
            let src = read_source(args.get(2));
            let entry_dir = entry_dir_of(args.get(2));

            // Create a multi-worker pool with pre-loaded definitions.
            // Workers load only definitions (FuncDef/ActorDef/TypeDef/EnumDef/Import),
            // not side-effect statements, so they don't re-run main logic.
            let n = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let pool = onely::runtime::worker::WorkerPool::new(
                n,
                Some(src.clone()),
                entry_dir.clone(),
            );
            onely::runtime::worker::set_global_pool(pool);

            let mut interp = onely::Interpreter::new();
            if let Some(dir) = &entry_dir {
                interp.set_entry_dir(dir.clone());
            }
            if let Err(e) = interp.run(&src) {
                eprintln!("{}", e.render(&src));
                std::process::exit(1);
            }
        }
        "selfvm" => {
            // Self-hosted VM: run the target file through the 1y-implemented
            // toolchain (bootstrap/{lexer,parser,compiler,vm}.1y). This is
            // Stage 5 of the self-bootstrapping process. The tree-walker
            // executes bootstrap/selfvm.1y, which loads the target file and
            // invokes vm.run_source(src).
            run_selfvm(args.get(2));
        }
        "vm" => {
            // Explicit VM subcommand. Same as the default path below.
            run_vm(args.get(2));
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
        // Default: treat the first arg as a file path (or `-` for stdin)
        // and run via the bytecode VM. The VM keeps call frames on the heap
        // (Vec<Frame>), so recursion depth is bounded only by available
        // memory — not by the native stack.
        _ => run_vm(args.get(1)),
    }
}

/// Run a target file via the self-hosted 1y VM.
///
/// This executes `bootstrap/selfvm.1y` through the tree-walking interpreter.
/// The target file path is passed via the `1Y_SELFVM_TARGET` environment
/// variable (read by selfvm.1y), since `env.args()` reflects the outer
/// process args and can't be easily overridden.
fn run_selfvm(target_arg: Option<&String>) {
    let target = match target_arg {
        Some(p) if p == "-" => {
            eprintln!("selfvm does not support stdin input");
            std::process::exit(2);
        }
        Some(p) => p.clone(),
        None => {
            eprintln!("missing file path");
            std::process::exit(2);
        }
    };

    // Resolve target to an absolute path so selfvm.1y can find it regardless
    // of its own working directory.
    let target_path = std::fs::canonicalize(&target).unwrap_or_else(|_| PathBuf::from(&target));
    std::env::set_var("1Y_SELFVM_TARGET", target_path.to_string_lossy().to_string());

    // Locate bootstrap/selfvm.1y relative to the current executable.
    // Fall back to a relative path if the executable-relative lookup fails.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let candidates = [
        exe_dir.join("bootstrap").join("selfvm.1y"),
        PathBuf::from("bootstrap").join("selfvm.1y"),
    ];
    let selfvm_path = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| candidates[1].clone());

    if !selfvm_path.exists() {
        eprintln!(
            "error: cannot find bootstrap/selfvm.1y (looked at {})",
            selfvm_path.display()
        );
        std::process::exit(2);
    }

    let selfvm_src = std::fs::read_to_string(&selfvm_path).unwrap_or_else(|e| {
        eprintln!("failed to read `{}`: {}", selfvm_path.display(), e);
        std::process::exit(2);
    });
    let entry_dir = selfvm_path.parent().map(|p| p.to_path_buf());

    // Set up the worker pool with the selfvm script pre-loaded (so workers
    // can call into vm.1y etc. without re-parsing).
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let pool = onely::runtime::worker::WorkerPool::new(
        n,
        Some(selfvm_src.clone()),
        entry_dir.clone(),
    );
    onely::runtime::worker::set_global_pool(pool);

    let mut interp = onely::Interpreter::new();
    if let Some(dir) = &entry_dir {
        interp.set_entry_dir(dir.clone());
    }

    if let Err(e) = interp.run(&selfvm_src) {
        eprintln!("{}", e.render(&selfvm_src));
        std::process::exit(1);
    }
}

/// Run source via the bytecode VM.
fn run_vm(path_arg: Option<&String>) {
    let src = read_source(path_arg);
    let entry_dir = entry_dir_of(path_arg);
    let mut vm = onely::vm::Vm::new();
    vm.register_builtins();
    if let Some(dir) = &entry_dir {
        vm.set_entry_dir(dir.clone());
    }
    match vm.run_source(&src) {
        Ok(v) => {
            // Print the final value if it's not nil (mirrors tree-walker
            // behaviour where the last top-level expr's value is shown).
            if !matches!(v, onely::value::Value::Nil) {
                println!("{}", v);
            }
        }
        Err(e) => {
            eprintln!("{}", e.render(&src));
            std::process::exit(1);
        }
    }
}

fn entry_dir_of(path_arg: Option<&String>) -> Option<PathBuf> {
    path_arg.and_then(|p| {
        if p == "-" {
            None
        } else {
            PathBuf::from(p).parent().map(|p| p.to_path_buf())
        }
    })
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
    eprintln!("    {} <file|->         parse and evaluate (bytecode VM, default)", prog);
    eprintln!("    {} vm <file|->      same as default (explicit)", prog);
    eprintln!("    {} run <file|->     parse and evaluate (tree-walking interpreter)", prog);
    eprintln!("    {} selfvm <file>    run via the self-hosted 1y VM (Stage 5)", prog);
    eprintln!("    {} parse <file|->   parse source, print the AST", prog);
    eprintln!("    {} tokens <file|->  print the token stream", prog);
    eprintln!("    {} help             show this message", prog);
}
