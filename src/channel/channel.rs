use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ptr::NonNull;

use crate::channel::builder::Builder;
use crate::error::{TalariaError, TalariaResult};
use crate::partition::{
    Concurrent,
    Exclusive,
    Partition,
    PartitionMode,
    PartitionState,
    PartitionT,
};
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
    /// ```ignore
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
        Partition::<Exclusive, T>::new(partition_id, self.inner.clone(), &self.partition_state)
    }

    /// Acquires access to the specified concurrent partition
    ///
    /// If the partition ID is invalid, or the cited partition is not concurrent, an error is
    /// returned.
    pub fn get_concurrent_partition(
        &self,
        partition_id: usize,
    ) -> TalariaResult<Partition<Concurrent, T>> {
        Partition::<Concurrent, T>::new(partition_id, self.inner.clone(), &self.partition_state)
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
    ring_ptr: NonNull<UnsafeCell<T>>,
    ring_size: usize,
    partition_state: PartitionState,
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

        if data.len() > (isize::MAX / 2) as usize {
            return Err(TalariaError::ElementsTooLarge {
                count: data.len(),
            });
        }

        if data.is_empty() {
            return Err(TalariaError::NoElements);
        }

        let partition_state = PartitionState::new(partition_definitions);

        let data = data
            .into_iter()
            .map(|elem| UnsafeCell::new(elem))
            .collect::<Vec<_>>();

        let mut data = ManuallyDrop::new(data);
        // shrink to fit so we only need to track length instead of length + capacity
        data.shrink_to_fit();

        let (ring_ptr, ring_size) = (NonNull::new(data.as_mut_ptr()).unwrap(), data.len());

        Ok(Self {
            ring_ptr,
            ring_size,
            partition_state,
        })
    }

    pub(crate) fn ring_ptr(&self) -> NonNull<UnsafeCell<T>> {
        self.ring_ptr
    }

    pub fn len(&self) -> usize {
        self.ring_size
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
    use crate::partition::PartitionState;

    impl<T> Inner<T> {
        pub fn introspect_partition_state(&self) -> &PartitionState {
            &self.partition_state
        }
    }
}
