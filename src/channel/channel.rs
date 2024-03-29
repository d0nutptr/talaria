use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ptr::NonNull;

use crate::channel::builder::Builder;
use crate::error::{TalariaError, TalariaResult};
use crate::partition::{Concurrent, Exclusive, Partition, PartitionMode};
use crate::sync_types::sync::Arc;

/// Manages a fixed collection of objects and partitions to manage these objects.
#[derive(Debug, Clone)]
pub struct Channel<T> {
    inner: Arc<Inner<T>>,
}

#[cfg(test)]
impl<T> Channel<T> {
    pub fn introspect_get_inner(&self) -> &Inner<T> {
        &self.inner
    }
}

impl<T> Channel<T> {
    pub(crate) fn new(
        elements: Vec<T>,
        partition_definitions: Vec<PartitionMode>,
    ) -> TalariaResult<Self> {
        let inner = Inner::new(elements, partition_definitions)?;

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Creates a channel [Builder](Builder) to configure a new channel.
    /// ```
    /// # use talaria::channel::Channel;
    /// # let some_objects = vec![0, 1, 2, 3];
    /// let channel = Channel::builder()
    ///     .add_exclusive_partition()
    ///     .add_exclusive_partition()
    ///     .build(some_objects)
    ///     .unwrap();
    ///
    /// let partition = channel.get_exclusive_partition(0).unwrap();
    /// ```
    pub fn builder() -> Builder<T> {
        Builder::new()
    }

    /// Acquires access to the specified exclusive partition
    ///
    /// If the partition ID is invalid, or the cited partition is not exclusive, or this exclusive
    /// partition is currently in use, an error is returned.
    pub fn get_exclusive_partition(
        &self,
        partition_id: usize,
    ) -> TalariaResult<Partition<Exclusive, T>> {
        self.inner.get_exclusive_partition(partition_id)
    }

    /// Acquires access to the specified concurrent partition
    ///
    /// If the partition ID is invalid, or the cited partition is not concurrent, an error is
    /// returned.
    pub fn get_concurrent_partition(
        &self,
        partition_id: usize,
    ) -> TalariaResult<Partition<Concurrent, T>> {
        self.inner.get_concurrent_partition(partition_id)
    }
}

impl<T> Deref for Channel<T> {
    type Target = Arc<Inner<T>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Debug)]
pub struct Inner<T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    partition_states: Vec<crate::partition::PartitionState>,
}

impl<T> Inner<T> {
    pub(crate) fn new(
        data: Vec<T>,
        partition_definitions: Vec<PartitionMode>,
    ) -> TalariaResult<Self> {
        const MIN_PARTITIONS: usize = 2; // todo: can this be 1?

        assert_ne!(
            std::mem::size_of::<T>(),
            0,
            "zero-sized types not supported"
        );

        if partition_definitions.len() < MIN_PARTITIONS {
            return Err(TalariaError::TooFewPartitions {
                requested: partition_definitions.len(),
            });
        }

        if !data.len().is_power_of_two() {
            return Err(TalariaError::ElementsNotPowerOfTwo {
                count: data.len(),
            });
        }

        if data.is_empty() {
            return Err(TalariaError::NoElements);
        }

        let (mut partition_states, ptrs): (Vec<_>, Vec<_>) = partition_definitions
            .into_iter()
            .enumerate()
            .map(|(id, mode)| crate::partition::PartitionState::new(id, mode))
            .map(|state| {
                let ptr = state.inner_ptr();

                (state, ptr)
            })
            .unzip();

        // create an iterator that is rotated backward by 1 (which is done by cycling
        // and skipping the last element) this is so that we can pass the
        // boundary partition index to the appropriate partition state builder
        // partition 1 should get partition 0's committed index pointer, partition 2
        // should get partition 1's committed index pointer, etc.
        // notably, partition 0 should get partition N's committed index pointer
        let offset_committed_indexes = ptrs.into_iter().cycle().skip(partition_states.len() - 1);

        partition_states
            .iter_mut()
            .zip(offset_committed_indexes)
            .for_each(|(partition_state, boundary_state)| {
                partition_state.set_boundary_state(boundary_state);
            });

        let mut data = ManuallyDrop::new(data);
        // shrink to fit so we only need to track length instead of length + capacity
        data.shrink_to_fit();

        let (ptr, len) = (NonNull::new(data.as_mut_ptr()).unwrap(), data.len());

        Ok(Self {
            ring_ptr: ptr,
            ring_size: len,
            partition_states,
        })
    }

    pub(crate) fn ring_ptr(&self) -> NonNull<T> {
        self.ring_ptr
    }

    pub fn len(&self) -> usize {
        self.ring_size
    }

    pub fn get_exclusive_partition(
        &self,
        partition_id: usize,
    ) -> TalariaResult<Partition<Exclusive, T>> {
        let partition_state =
            self.partition_states
                .get(partition_id)
                .ok_or(TalariaError::PartitionNotFound {
                    partition_id,
                })?;

        Partition::<Exclusive, T>::new(self.ring_ptr(), self.len(), partition_state)
    }

    pub fn get_concurrent_partition(
        &self,
        partition_id: usize,
    ) -> TalariaResult<Partition<Concurrent, T>> {
        let partition_state =
            self.partition_states
                .get(partition_id)
                .ok_or(TalariaError::PartitionNotFound {
                    partition_id,
                })?;

        Partition::<Concurrent, T>::new(self.ring_ptr(), self.len(), partition_state)
    }
}

unsafe impl<T> Send for Inner<T> where Box<T>: Send {}
unsafe impl<T> Sync for Inner<T> where Box<T>: Sync {}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.ring_ptr.as_ptr(), self.ring_size, self.ring_size);
        }
    }
}

#[cfg(any(test, loom))]
mod test_utils {
    use crate::channel::channel::Inner;

    impl<T> Inner<T> {
        pub fn introspect_partition_states(&self) -> &Vec<crate::partition::PartitionState> {
            &self.partition_states
        }
    }
}
