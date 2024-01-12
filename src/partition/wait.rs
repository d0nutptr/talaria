use std::cell::RefCell;
use std::hash::Hash;
use std::sync::atomic::AtomicUsize as StdAtomicUsize;

use crate::sync_types::sync::atomic::{AtomicBool, Ordering};
use crate::sync_types::sync::Mutex;
use crate::sync_types::thread::{current as current_thread, park, Thread};

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub(crate) struct Token(u128);

impl Token {
    pub(crate) fn new() -> Self {
        // if the id is u128, then shift 64 bits left
        const THREAD_ID_OFFSET: usize = 64;
        static THREAD_COUNTER: StdAtomicUsize = StdAtomicUsize::new(0);

        thread_local! {
            // do this because ThreadId.as_u64() is unstable
            static THREAD_ID: u128 = THREAD_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
            static COUNTER: RefCell<u128> = RefCell::new(0);
        }

        let thread_id = THREAD_ID.with(|thread_id| *thread_id);
        let thread_local_counter = COUNTER.with(|counter| {
            let mut counter = counter.borrow_mut();
            *counter += 1;
            *counter
        });
        let global_counter = thread_id << THREAD_ID_OFFSET | thread_local_counter;

        Self(global_counter)
    }
}

// todo: make multiple strategies, including an 'async wait strategy' so it can
// be used in tokio maybe todo: maybe this should have clients register for the
// wait using atomics
#[derive(Debug)]
pub struct BlockingWaitStrategy {
    listeners: Mutex<Vec<(Token, Thread)>>,
    is_empty: AtomicBool,
}

pub trait WaitStrategy {
    /// will wait until notified or 100ms, whichever comes first
    fn wait(&self);

    /// if listeners are registered, notifies all of them
    fn notify(&self);

    fn register(&self, token: &Token);

    fn unregister(&self, token: &Token);
}

impl BlockingWaitStrategy {
    pub fn new() -> Self {
        // honestly this could be larger, but 16 is probably as many listeners are we
        // might expect to have in any reasonable application
        const INITIAL_LISTENER_CAPACITY: usize = 16;

        Self {
            listeners: Mutex::new(Vec::with_capacity(INITIAL_LISTENER_CAPACITY)),
            is_empty: AtomicBool::new(true),
        }
    }
}

impl WaitStrategy for BlockingWaitStrategy {
    fn wait(&self) {
        park()
    }

    fn notify(&self) {
        // shamelessly taken from mpmc in the std library
        // https://github.com/rust-lang/rust/blob/e51e98dde6a60637b6a71b8105245b629ac3fe77/library/std/src/sync/mpmc/waker.rs#L173
        if !self.is_empty.load(Ordering::SeqCst) {
            let mut listeners = self.listeners.lock().unwrap();

            if !self.is_empty.load(Ordering::SeqCst) {
                // todo: check if we should do what the std library does and only notify one
                // thread or if we should notify all of them like we do here
                for (_, thread) in listeners.drain(..) {
                    thread.unpark();
                }

                self.is_empty.store(listeners.is_empty(), Ordering::SeqCst);
            }
        }
    }

    fn register(&self, token: &Token) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.push((*token, current_thread()));
        self.is_empty.store(listeners.is_empty(), Ordering::SeqCst);
    }

    fn unregister(&self, token: &Token) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.retain(|(t, _)| *t != *token);
    }
}
