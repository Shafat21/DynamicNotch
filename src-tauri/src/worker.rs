//! Dedicated threads that run closures sent to them (sound, brightness): each owns its OS state
//! (COM on Windows) for the life of the app, and serializes access to the hardware.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

pub type Job = Box<dyn FnOnce() + Send>;

/// Run one job; a panic is logged (once) and swallowed so the worker loop survives. The panicked job's
/// caller gets an Err because its reply sender is dropped unsent. Release builds unwind too (no `panic = "abort"`).
pub fn run_job(job: Job) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if catch_unwind(AssertUnwindSafe(job)).is_err() && !LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("worker: a job panicked; the worker keeps running");
    }
}

/// A plain worker: `init` once on the new thread, then jobs in order until the app exits.
pub fn spawn(init: fn()) -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        init();
        for job in rx {
            run_job(job);
        }
    });
    tx
}

/// Run `f` on the worker behind `jobs` and wait for its result.
pub fn call<T: Send + 'static>(jobs: Option<&mpsc::Sender<Job>>, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    let stopped = || "This feature is not available right now".to_string();
    jobs.ok_or_else(stopped)?.send(Box::new(move || drop(tx.send(f())))).map_err(|_| stopped())?;
    rx.recv().map_err(|_| stopped())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survives_a_panicking_job() {
        let jobs = spawn(|| {});
        let r: Result<(), String> = call(Some(&jobs), || panic!("boom"));
        assert!(r.is_err()); // the caller hears about it
        assert_eq!(call(Some(&jobs), || Ok(7)), Ok(7)); // and the worker is still alive
        assert!(call::<()>(None, || Ok(())).is_err()); // not started
    }
}
