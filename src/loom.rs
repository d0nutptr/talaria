#[cfg(not(loom))]
pub(crate) use std::{
    sync,
    thread,
    hint,
};

#[cfg(loom)]
pub(crate) use loom::{
    sync,
    thread,
    hint,
};