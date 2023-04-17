//! A silly POC of a way to offload arbitrary tasks to a worker thread
//! (e.g., to serialize them without breaking out `async` machinery)
//! and await their results.

use std::sync::mpsc::{channel, sync_channel, Receiver, RecvError, Sender, TryRecvError};

/// Arbitrary work we can enqueue in a channel and then call in another thread
/// on the receiving end.
pub type PackagedTask = Box<dyn FnOnce() + Send>;

/// Packages jobs into something that can be awaited with a [`Future`]
#[derive(Debug)]
pub struct TaskSender {
    /// Tasks to do.
    ///
    /// By the time they get here, they're type-erased into any closure
    /// we can send.
    todos: Sender<PackagedTask>,
}

/// Something that can be waited on in order to produce a result
/// computed by calls to [`tick()`]
#[derive(Debug)]
pub struct Future<T> {
    // This could maybe be something simpler, but IIRC
    // std::sync::mpsc (now with crossbeam!) provides a pretty lightweight
    // channel implementation if it's a bounded chnanel of size 1.
    rx: Receiver<T>,
}

impl<T> Future<T> {
    /// Returns the result, or a [`RecvError`] if this outlived the [`TaskSender`]
    pub fn wait(self) -> Result<T, RecvError> {
        self.rx.recv()
    }
}

impl TaskSender {
    pub fn new() -> (Self, Receiver<PackagedTask>) {
        let (todos, rx) = channel::<PackagedTask>();
        (Self { todos }, rx)
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
            // We don't care if the future gets its result;
            // if send() fails there's nothing we can do from here.
            let _ = tx.send(fun());
        };
        // We should always be able to send to the worker unless it crashed
        self.todos
            .send(Box::new(erased))
            .expect("Worker thread hung up");
        Future { rx }
    }

    /// Push work onto the worker thread and immediately wait for it to finish.
    ///
    /// Useful for other threads to serialize work on a worker calling [`tick()`]
    pub fn run<T, F>(&self, fun: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        // We shouldn't get an error here since `wait()`
        // only fails if the future outlives the worker,
        // and here we are in the worker.
        self.send(fun).wait().unwrap()
    }
}

/// Run a single packaged task, presumably on a worker thread.
///
/// Return an error if the [`TaskSender`] hung up or there's nothing to do.
pub fn tick(rx: &Receiver<PackagedTask>) -> Result<(), RecvError> {
    rx.recv().map(|job| job())
}

/// Run a single packaged task if one is ready, presumably on a worker thread.
///
/// Return an error if the [`TaskSender`] hung up or there's nothing to do.
pub fn try_tick(rx: &Receiver<PackagedTask>) -> Result<(), TryRecvError> {
    rx.try_recv().map(|job| job())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn smoke() {
        let (tx, rx) = TaskSender::new();
        let future = tx.send(|| 42 + 27);
        drop(tx);

        tick(&rx).unwrap();
        assert_eq!(69, future.wait().unwrap())
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

        let (tx, rx) = TaskSender::new();

        // Run tick() in a worker thread.
        let t = std::thread::spawn(move || while tick(&rx).is_ok() {});

        let mut results = vec![];
        for d in &data {
            let d = *d;
            // Some dumb function that returns the given value
            let jorb = move || d;
            results.push(tx.run(jorb));
        }
        drop(tx);

        t.join().unwrap();

        assert_eq!(results, data);
    }
}
