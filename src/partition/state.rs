use std::mem::ManuallyDrop;
use std::ptr::NonNull;

use crossbeam_utils::CachePadded;

use crate::error::{TalariaError, TalariaResult};
use crate::partition::mode::PartitionMode;
use crate::partition::wait::BlockingWaitStrategy;
use crate::sync_types::sync::atomic::AtomicUsize;

#[derive(Debug)]
pub struct PartitionState {
    committed_index_ptr: NonNull<CachePadded<AtomicUsize>>,
    reserved_index_ptr: NonNull<CachePadded<AtomicUsize>>,
    waker_ptr: NonNull<CachePadded<BlockingWaitStrategy>>,
    partition_modes: Vec<PartitionMode>,
}

impl PartitionState {
    pub fn new(partition_modes: Vec<PartitionMode>) -> Self {
        let committed_index = (0..partition_modes.len())
            .map(|_| CachePadded::new(AtomicUsize::new(0)))
            .collect::<Vec<_>>();

        let reserved_index = (0..partition_modes.len())
            .map(|_| CachePadded::new(AtomicUsize::new(0)))
            .collect::<Vec<_>>();

        let wakers = (0..partition_modes.len())
            .map(|_| CachePadded::new(BlockingWaitStrategy::new()))
            .collect::<Vec<_>>();

        let mut committed_index = ManuallyDrop::new(committed_index);
        let mut reserved_index = ManuallyDrop::new(reserved_index);
        let mut wakers = ManuallyDrop::new(wakers);

        committed_index.shrink_to_fit();
        reserved_index.shrink_to_fit();
        wakers.shrink_to_fit();

        let committed_index_ptr = NonNull::new(committed_index.as_mut_ptr()).unwrap();
        let reserved_index_ptr = NonNull::new(reserved_index.as_mut_ptr()).unwrap();
        let waker_ptr = NonNull::new(wakers.as_mut_ptr()).unwrap();

        Self {
            committed_index_ptr,
            reserved_index_ptr,
            waker_ptr,
            partition_modes,
        }
    }

    #[inline]
    pub(crate) unsafe fn committed_index(
        &self,
        partition_id: usize,
    ) -> NonNull<CachePadded<AtomicUsize>> {
        self.committed_index_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) unsafe fn reserved_index(
        &self,
        partition_id: usize,
    ) -> NonNull<CachePadded<AtomicUsize>> {
        self.reserved_index_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) unsafe fn boundary_index(
        &self,
        partition_id: usize,
    ) -> NonNull<CachePadded<AtomicUsize>> {
        let shifted_index = if partition_id == 0 {
            self.partition_modes.len() - 1
        } else {
            partition_id - 1
        };

        self.committed_index_ptr.add(shifted_index)
    }

    #[inline]
    pub(crate) unsafe fn waker(
        &self,
        partition_id: usize,
    ) -> NonNull<CachePadded<BlockingWaitStrategy>> {
        self.waker_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) fn partition_mode(&self, partition_id: usize) -> TalariaResult<&PartitionMode> {
        self.partition_modes
            .get(partition_id)
            .ok_or(TalariaError::PartitionNotFound {
                partition_id,
            })
    }
}

impl Drop for PartitionState {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(
                self.committed_index_ptr.as_ptr(),
                self.partition_modes.len(),
                self.partition_modes.len(),
            );
            Vec::from_raw_parts(
                self.reserved_index_ptr.as_ptr(),
                self.partition_modes.len(),
                self.partition_modes.len(),
            );
            Vec::from_raw_parts(
                self.waker_ptr.as_ptr(),
                self.partition_modes.len(),
                self.partition_modes.len(),
            );
        }
    }
}

#[cfg(any(test, loom, shuttle))]
mod test_utils {
    use crate::partition::PartitionState;

    impl PartitionState {
        pub fn introspect_partition_modes(&self) -> &[super::PartitionMode] {
            &self.partition_modes
        }

        pub fn introspect_committed_index(&self, offset: usize) -> usize {
            if offset >= self.partition_modes.len() {
                panic!("Offset {} is out of bounds", offset)
            }

            unsafe {
                self.committed_index(offset)
                    .as_ref()
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }
        pub fn introspect_reserved_index(&self, offset: usize) -> usize {
            if offset >= self.partition_modes.len() {
                panic!("Offset {} is out of bounds", offset)
            }

            unsafe {
                self.reserved_index(offset)
                    .as_ref()
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }
        pub fn introspect_boundary_index(&self, offset: usize) -> usize {
            if offset >= self.partition_modes.len() {
                panic!("Offset {} is out of bounds", offset)
            }

            unsafe {
                self.boundary_index(offset)
                    .as_ref()
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }
    }
}
