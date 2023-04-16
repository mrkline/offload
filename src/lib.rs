//! A silly POC of a way to offload arbitrary tasks to a worker thread
//! (e.g., to serialize them without breaking out `async` machinery)
//! and await their results.

use std::{
    sync::mpsc::{channel, sync_channel, Receiver, RecvError, Sender},
    thread::{self, spawn, JoinHandle},
};

/// A worker thread to send arbitray tasks to, so long as they're [`Send`]
#[derive(Debug)]
pub struct Worker {
    /// Handle for the worker thread
    t: JoinHandle<()>,

    /// Tasks to do.
    ///
    /// By the time they get here, they're type-erased into any closure
    /// we can send.
    todos: Sender<Box<dyn FnOnce() + Send>>,
}

/// Something that can be waited on in order to produce a result
/// computed by a [`Worker`] job
#[derive(Debug)]
pub struct Future<T> {
    // This could maybe be something simpler, but IIRC
    // std::sync::mpsc (now with crossbeam!) provides a pretty lightweight
    // channel implementation if it's a bounded chnanel of size 1.
    rx: Receiver<T>,
}

impl<T> Future<T> {
    /// Returns the result, or a [`RecvError`] if this outlived the [`Worker`]
    pub fn wait(self) -> Result<T, RecvError> {
        self.rx.recv()
    }
}

impl Worker {
    pub fn new() -> Self {
        let (todos, rx) = channel::<Box<dyn FnOnce() + Send>>();
        let worker = spawn(move ||
            // Just drain the queue
            while let Ok(work) = rx.recv(){
                work();
            }
        );
        Self { t: worker, todos }
    }

    /// Join the worker thread. (The default [`Drop`] detaches it.)
    pub fn join(self) -> thread::Result<()> {
        self.t.join()
    }

    /// Push work onto the worker thread,
    /// returning a [`Future`] that can await the result.
    pub fn send<T, F>(&self, fun: F) -> Future<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (tx, rx) = sync_channel(1);
        // Type erasure! Put our closure in a closure that sends the result.
        let erased = move || {
            match tx.send(fun()) {
                Ok(()) => { /* The future gets its result */ }
                Err(_e) => {
                    // If the future got closed,
                    // there's nothing we can do from here.
                }
            }
        };
        // We should always be able to send to the worker unless it crashed
        self.todos
            .send(Box::new(erased))
            .expect("Worker thread hung up");
        Future { rx }
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn smoke() {
        let w = Worker::new();
        let future = w.send(|| 42 + 27);
        assert_eq!(69, future.wait().unwrap());
    }

    #[test]
    fn sequential() {
        let data = [
            "Every gun that is made",
            "every warship launched",
            "every rocket fired signifies",
            "in the final sense",
            "a theft from those who hunger and are not fed",
            "those who are cold and are not clothed.",
            "This world in arms is not spending money alone.",
        ];
        let mut futures = vec![];

        let w = Worker::new();
        for d in &data {
            let d = *d;
            // Some dumb function that returns the given value
            let jorb = move || d;
            futures.push(w.send(jorb));
        }

        let results: Vec<_> = futures.into_iter().map(|f| f.wait().unwrap()).collect();

        assert_eq!(results, data);
    }
}
