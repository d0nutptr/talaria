#[cfg(not(any(loom, shuttle)))]
pub(crate) use std::{
    sync,
    thread,
    hint,
};

#[cfg(all(loom, not(shuttle)))]
pub(crate) use loom::{
    sync,
    thread,
    hint,
};

#[cfg(all(shuttle, not(loom)))]
pub(crate) use shuttle::{
    sync,
    thread,
    hint
};