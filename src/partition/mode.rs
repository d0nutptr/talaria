use crate::sync_types::sync::atomic::AtomicBool;
use crate::sync_types::sync::Arc;

#[derive(Debug, Clone)]
pub enum PartitionMode {
    /// Marks a partition as "exclusive" which means that only one consumer can access it at a time.
    Exclusive { in_use: Arc<AtomicBool> },

    /// Marks a partition as "concurrent" which means that multiple consumers can access it simultaneously.
    Concurrent,
}

impl PartitionMode {
    pub fn with_exclusive_access() -> PartitionMode {
        PartitionMode::Exclusive {
            in_use: Default::default(),
        }
    }

    pub fn with_concurrent_access() -> PartitionMode {
        PartitionMode::Concurrent
    }
}

/// Marks a partition as "exclusive" which means that only one consumer can
/// access it at a time.
///
/// This mode uses a fast, non-atomic reservation system.
#[derive(Debug)]
pub struct Exclusive {
    pub(crate) in_use: Arc<AtomicBool>,
}

/// Marks a partition as "concurrent" which means that multiple consumers can
/// access it at a time
///
/// This mode uses a slower, though still efficient, atomic reservation system.
#[derive(Debug, Clone)]
pub struct Concurrent;

trait Sealed {}
impl Sealed for Exclusive {}
impl Sealed for Concurrent {}

#[allow(private_bounds)]
pub trait PartitionModeT: Sealed {}
impl<T> PartitionModeT for T where T: Sealed {}
