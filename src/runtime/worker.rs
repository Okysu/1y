//! Worker thread pool for multi-core parallelism (Phase C2 + multi-thread).
//!
//! Each worker thread runs its own independent `Interpreter` + `Scheduler` +
//! `mio::Poll`. Workers pre-load the entry file's definitions (functions,
//! actors, types, imports) so they can execute `parallel.call("func", args)`
//! without re-parsing the entry file each time.
//!
//! Cross-thread communication uses `SendValue` (a Send+Sync subset of Value).

use crate::interpreter::Interpreter;
use crate::value::SendValue;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Barrier, LazyLock, Mutex};

/// A job submitted to the worker pool: function name + args + reply channel.
struct Job {
    func_name: String,
    args: Vec<SendValue>,
    reply: Sender<Result<SendValue, String>>,
}

struct WorkerPoolShared {
    job_tx: Sender<Job>,
}

/// A pool of worker threads, each running its own `Interpreter`.
pub struct WorkerPool {
    shared: Arc<WorkerPoolShared>,
}

/// Global worker pool, set by main.rs before user code runs.
static GLOBAL_POOL: LazyLock<Mutex<Option<Arc<WorkerPool>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Set the global worker pool. Called by main.rs during startup.
pub fn set_global_pool(pool: Arc<WorkerPool>) {
    *GLOBAL_POOL.lock().unwrap() = Some(pool);
}

/// Get a clone of the global worker pool, if initialized.
pub fn get_global_pool() -> Option<Arc<WorkerPool>> {
    GLOBAL_POOL.lock().unwrap().as_ref().cloned()
}

impl WorkerPool {
    /// Create a pool with `n` workers. Each worker pre-loads definitions from
    /// `entry_source` (if provided), then waits for `call` requests.
    /// Returns when all workers have finished pre-loading.
    pub fn new(
        n: usize,
        entry_source: Option<String>,
        entry_dir: Option<PathBuf>,
    ) -> Arc<Self> {
        assert!(n >= 1, "worker pool must have at least 1 worker");
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let barrier = Arc::new(Barrier::new(n + 1)); // n workers + main

        for _ in 0..n {
            let job_rx = job_rx.clone();
            let src = entry_source.clone();
            let dir = entry_dir.clone();
            let b = barrier.clone();
            std::thread::Builder::new()
                .stack_size(256 * 1024 * 1024)
                .spawn(move || {
                    let mut interp = Interpreter::new();
                    if let Some(dir) = &dir {
                        interp.set_entry_dir(dir.clone());
                    }
                    // Pre-load definitions (ignore errors — worker stays
                    // alive but calls will fail with "not defined").
                    if let Some(src) = &src {
                        let _ = interp.load_definitions(src);
                    }
                    // Signal: pre-loading done.
                    b.wait();

                    // Job loop.
                    loop {
                        let job = {
                            let rx = job_rx.lock().unwrap();
                            rx.recv()
                        };
                        match job {
                            Ok(job) => {
                                let result = interp
                                    .call_function_by_name(&job.func_name, job.args);
                                let _ = job.reply.send(result);
                            }
                            Err(_) => break, // channel closed, exit
                        }
                    }
                })
                .expect("failed to spawn worker thread");
        }

        // Wait for all workers to finish pre-loading.
        barrier.wait();

        Arc::new(WorkerPool {
            shared: Arc::new(WorkerPoolShared { job_tx }),
        })
    }

    /// Submit a function call to the pool. Returns a receiver that yields
    /// the result when the call completes.
    pub fn call(
        &self,
        func_name: String,
        args: Vec<SendValue>,
    ) -> Receiver<Result<SendValue, String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.shared
            .job_tx
            .send(Job { func_name, args, reply: reply_tx })
            .expect("worker pool channel closed");
        reply_rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_creates_and_calls() {
        let src = "fn double(n) { n * 2 }".to_string();
        let pool = WorkerPool::new(2, Some(src), None);
        let rx = pool.call("double".into(), vec![SendValue::Int(21.into())]);
        let result = rx.recv().expect("reply channel closed");
        match result {
            Ok(SendValue::Int(n)) => assert_eq!(n, 42.into()),
            other => panic!("expected Int(42), got {:?}", other),
        }
    }

    #[test]
    fn test_pool_multiple_workers_parallel() {
        let src = "fn slow_compute(n) { let s = 0; let i = 0; while i < n { s = s + i; i = i + 1 }; s }".to_string();
        let pool = WorkerPool::new(4, Some(src), None);
        let mut receivers = Vec::new();
        for _ in 0..4 {
            let rx = pool.call("slow_compute".into(), vec![SendValue::Int(100000.into())]);
            receivers.push(rx);
        }
        for rx in receivers {
            let result = rx.recv().expect("reply channel closed");
            assert!(result.is_ok(), "call failed: {:?}", result);
        }
    }

    #[test]
    fn test_pool_function_not_defined() {
        let pool = WorkerPool::new(1, Some("fn exists() { 1 }".into()), None);
        let rx = pool.call("nonexistent".into(), vec![]);
        let result = rx.recv().expect("reply channel closed");
        assert!(result.is_err());
    }

    #[test]
    fn test_pool_undefined_function_error_message() {
        let pool = WorkerPool::new(1, None, None);
        let rx = pool.call("missing".into(), vec![]);
        let result = rx.recv().expect("reply channel closed");
        let err = result.unwrap_err();
        assert!(err.contains("not defined") || err.contains("missing"));
    }
}
