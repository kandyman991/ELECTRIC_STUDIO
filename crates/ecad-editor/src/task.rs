//! Long operations with progress and cancellation (D34): verification of a large folder, a
//! big change set's preview. A task runs on its own thread; the UI polls its progress each
//! frame and may ask it to stop. A cancelled task changes nothing.

use ecad_project::state::Progress;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Shared between the task and whoever watches it.
#[derive(Debug, Default)]
pub struct Shared {
    cancel: AtomicBool,
    done: AtomicUsize,
    total: AtomicUsize,
    what: Mutex<String>,
}

impl Progress for Shared {
    fn step(&self, done: usize, total: usize, what: &str) {
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        if let Ok(mut w) = self.what.lock() {
            what.clone_into(&mut w);
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Shared {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    /// Done, total, and what it is on.
    pub fn progress(&self) -> (usize, usize, String) {
        let what = self.what.lock().map(|w| w.clone()).unwrap_or_default();
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed), what)
    }
}

/// A running long operation.
#[derive(Debug)]
pub struct Task<T> {
    pub title: String,
    shared: Arc<Shared>,
    handle: Option<std::thread::JoinHandle<T>>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(title: &str, f: impl FnOnce(&dyn Progress) -> T + Send + 'static) -> Task<T> {
        let shared = Arc::new(Shared::default());
        let s = Arc::clone(&shared);
        let handle = std::thread::spawn(move || f(s.as_ref()));
        Task { title: title.to_owned(), shared, handle: Some(handle) }
    }
    pub fn cancel(&self) {
        self.shared.cancel();
    }
    pub fn progress(&self) -> (usize, usize, String) {
        self.shared.progress()
    }
    pub fn finished(&self) -> bool {
        self.handle.as_ref().is_none_or(std::thread::JoinHandle::is_finished)
    }
    /// Its result, waiting for it; `None` if the thread panicked.
    pub fn join(mut self) -> Option<T> {
        self.handle.take().and_then(|h| h.join().ok())
    }
}
