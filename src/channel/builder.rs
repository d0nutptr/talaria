use std::marker::PhantomData;
use crate::channel::channel::Channel;
use crate::error::TalariaResult;
use crate::partition::PartitionMode;

#[derive(Debug, Clone)]
pub struct Builder<T> {
    partition_definitions: Vec<PartitionMode>,
    kind: PhantomData<T>
}

impl<T> Builder<T> {
    pub fn new() -> Self {
        Self {
            partition_definitions: Vec::new(),
            kind: PhantomData
        }
    }

    pub fn add_partition(&mut self, mode: PartitionMode) -> &mut Self {
        self.partition_definitions.push(mode);
        self
    }

    pub fn add_exclusive_partition(&mut self) -> &mut Self {
        self.add_partition(PartitionMode::with_exclusive_access())
    }

    pub fn add_concurrent_partition(&mut self) -> &mut Self {
        self.add_partition(PartitionMode::with_concurrent_access())
    }

    pub fn build(&self, elements: Vec<T>) -> TalariaResult<Channel<T>> {
        Channel::new(elements, self.partition_definitions.clone())
    }
}