use std::marker::PhantomData;

use crate::channel::channel::Channel;
use crate::error::TalariaResult;
use crate::partition::PartitionMode;

/// A builder for a [Channel](Channel).
///
/// See [Channel::builder](Channel::builder).
#[derive(Debug, Clone)]
pub struct Builder<T> {
    partition_definitions: Vec<PartitionMode>,
    kind: PhantomData<T>,
}

impl<T> Builder<T> {
    pub fn new() -> Self {
        Self {
            partition_definitions: Vec::new(),
            kind: PhantomData,
        }
    }

    /// Adds a new partition with the specified mode.
    ///
    /// The order partitions are added to the channel sets their partition ID starting with 0.
    pub fn add_partition(&mut self, mode: PartitionMode) -> &mut Self {
        self.partition_definitions.push(mode);
        self
    }

    /// Adds a new partition with exclusive access.
    ///
    /// The order partitions are added to the channel sets their partition ID starting with 0.
    pub fn add_exclusive_partition(&mut self) -> &mut Self {
        self.add_partition(PartitionMode::with_exclusive_access())
    }

    /// Adds a new partition with concurrent access.
    ///
    /// The order partitions are added to the channel sets their partition ID starting with 0.
    pub fn add_concurrent_partition(&mut self) -> &mut Self {
        self.add_partition(PartitionMode::with_concurrent_access())
    }

    pub fn build(&self, elements: Vec<T>) -> TalariaResult<Channel<T>> {
        Channel::new(elements, self.partition_definitions.clone())
    }
}
