#[cfg(not(any(loom, shuttle)))]
pub(crate) use std::{hint, sync, thread};

#[cfg(all(loom, not(shuttle)))]
pub(crate) use loom::{hint, sync, thread};
#[cfg(all(shuttle, not(loom)))]
pub(crate) use shuttle::{hint, sync, thread};
