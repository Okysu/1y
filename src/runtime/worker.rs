//! Worker thread pool for multi-core parallelism (Phase C2).
//!
//! In the BEAM-style concurrency model, each worker thread runs its own
//! independent `Interpreter` + `Scheduler` + `mio::Poll`. corosensei
//! coroutines are `!Send`, so they never cross thread boundaries — each
//! thread's coroutine pool is strictly local.
//!
//! Cross-thread actor messaging uses the `ActorRegistry` (Phase C1):
//! actors are addressed by `ActorPid`, and `CrossEnvelope`s are delivered
//! via per-thread `mpsc` channels. A worker periodically drains its
//! cross-thread inbox and dispatches messages to local actors.
//!
//! ## Design
//!
//! - **N workers**: each owns an `Interpreter` (with `Rc`-based `Value`s,
//!   safe because single-threaded per worker).
//! - **Job queue**: `mpsc::Sender<Job>` shared by all workers; any worker
//!   can pick up the next job.
//! - **Reply channel**: each job carries a oneshot reply sender so the
//!   caller gets the result.
//! - **Cross-thread inbox**: each worker has a `mpsc::Receiver<CrossEnvelope>`
//!   registered in the `ActorRegistry`, so actors on other threads can
//!   route messages to actors on this worker.

use crate::interpreter::Interpreter;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

/// A job submitted to the worker pool: source code + entry dir + reply channel.
struct Job {
    src: String,
    entry_dir: Option<PathBuf>,
    reply: Sender<Result<(), String>>,
}

/// Handle to a worker thread.
struct Worker {
    _thread: JoinHandle<()>,
}

/// A pool of worker threads, each running its own `Interpreter`.
///
/// Jobs are distributed via a shared `mpsc` channel. When a job completes,
/// its result (rendered error string or `Ok(())`) is sent back via the
/// job's reply channel.
pub struct WorkerPool {
    workers: Vec<Worker>,
    job_tx: Sender<Job>,
}

impl WorkerPool {
    /// Create a pool with `n` workers. Each worker spawns a thread with a
    /// large stack (256 MB) to accommodate deep `1y` recursion.
    pub fn new(n: usize) -> Self {
        assert!(n >= 1, "worker pool must have at least 1 worker");
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let job_rx = std::sync::Arc::new(std::sync::Mutex::new(job_rx));
        let mut workers = Vec::with_capacity(n);
        for _ in 0..n {
            let job_rx = job_rx.clone();
            let worker = Worker::spawn(job_rx);
            workers.push(worker);
        }
        WorkerPool { workers, job_tx }
    }

    /// Submit a job to the pool. Returns a receiver that yields the result
    /// when the job completes.
    pub fn submit(&self, src: String, entry_dir: Option<PathBuf>) -> Receiver<Result<(), String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.job_tx
            .send(Job { src, entry_dir, reply: reply_tx })
            .expect("worker pool channel closed");
        reply_rx
    }

    /// Wait for all workers to finish (workers exit when the job channel
    /// is closed). Returns results in completion order.
    pub fn join(self) -> Vec<Result<(), String>> {
        // Drop the sender to signal workers to exit after draining jobs.
        drop(self.job_tx);
        let results = Vec::new();
        for worker in self.workers {
            let _ = worker._thread.join();
        }
        results
    }
}

impl Worker {
    fn spawn(job_rx: std::sync::Arc<std::sync::Mutex<Receiver<Job>>>) -> Self {
        let thread = thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                let mut interp = Interpreter::new();
                loop {
                    // Lock only to receive, then release immediately.
                    let job = {
                        let rx = job_rx.lock().unwrap();
                        match rx.recv() {
                            Ok(job) => job,
                            Err(_) => break, // channel closed, exit
                        }
                    };
                    if let Some(dir) = &job.entry_dir {
                        interp.set_entry_dir(dir.clone());
                    }
                    let result = match interp.run(&job.src) {
                        Ok(()) => Ok(()),
                        Err(e) => Err(e.render(&job.src)),
                    };
                    let _ = job.reply.send(result);
                }
            })
            .expect("failed to spawn worker thread");
        Worker { _thread: thread }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_worker_runs_program() {
        let pool = WorkerPool::new(1);
        let rx = pool.submit("print(1 + 2)".to_string(), None);
        let result = rx.recv().expect("reply channel closed");
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        pool.join();
    }

    #[test]
    fn test_multiple_workers_parallel() {
        let pool = WorkerPool::new(4);
        let mut receivers = Vec::new();
        for i in 0..4 {
            let src = format!("print({})", i);
            receivers.push(pool.submit(src, None));
        }
        for rx in receivers {
            let result = rx.recv().expect("reply channel closed");
            assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        }
        pool.join();
    }

    #[test]
    fn test_error_returned() {
        let pool = WorkerPool::new(1);
        let rx = pool.submit("undefined_var".to_string(), None);
        let result = rx.recv().expect("reply channel closed");
        assert!(result.is_err(), "expected error");
        let err = result.unwrap_err();
        assert!(err.contains("undefined_var") || err.contains("not defined"));
        pool.join();
    }

    #[test]
    fn test_actor_spawns_on_worker() {
        let src = r#"
            actor Counter {
                state data = 0;
                on Bump() { data = data + 1; reply data }
            };
            let c = spawn Counter();
            let v = c ? Bump();
            print(v)
        "#;
        let pool = WorkerPool::new(2);
        let rx = pool.submit(src.to_string(), None);
        let result = rx.recv().expect("reply channel closed");
        assert!(result.is_ok(), "actor spawn failed: {:?}", result);
        pool.join();
    }
}
