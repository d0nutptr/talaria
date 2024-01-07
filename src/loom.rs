#[cfg(not(loom))]
pub(crate) use std::{
    sync,
    thread
};

#[cfg(loom)]
pub(crate) use loom::{
    sync,
    thread
};