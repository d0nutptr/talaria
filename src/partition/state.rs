use std::mem::ManuallyDrop;
use std::ptr::NonNull;

use crossbeam_utils::CachePadded;
use crate::error::{TalariaError, TalariaResult};

use crate::partition::mode::PartitionMode;
use crate::partition::wait::BlockingWaitStrategy;
use crate::sync_types::sync::atomic::AtomicUsize;

#[derive(Debug)]
pub struct PartitionStateInner {
    pub committed_index: CachePadded<AtomicUsize>,
    pub reserved_index: CachePadded<AtomicUsize>,
    pub waker: CachePadded<BlockingWaitStrategy>,
    pub partition_id: CachePadded<usize>,
    pub mode: PartitionMode,
}

impl PartitionStateInner {
    pub(crate) fn new(partition_id: usize, mode: PartitionMode) -> Self {
        Self {
            partition_id: CachePadded::from(partition_id),
            committed_index: CachePadded::from(AtomicUsize::new(0)),
            reserved_index: CachePadded::from(AtomicUsize::new(0)),
            waker: CachePadded::from(BlockingWaitStrategy::new()),
            mode,
        }
    }

    pub(crate) fn committed_index(&self) -> &CachePadded<AtomicUsize> {
        &self.committed_index
    }

    pub(crate) fn reserved_index(&self) -> &CachePadded<AtomicUsize> {
        &self.reserved_index
    }
}

#[derive(Debug)]
pub struct PartitionState2 {
    committed_index_ptr: NonNull<CachePadded<AtomicUsize>>,
    reserved_index_ptr: NonNull<CachePadded<AtomicUsize>>,
    waker_ptr: NonNull<CachePadded<BlockingWaitStrategy>>,
    partition_modes: Vec<PartitionMode>
}

impl PartitionState2 {
    pub fn new(partition_modes: Vec<PartitionMode>) -> Self {
        let committed_index = (0 .. partition_modes.len())
            .map(|_| CachePadded::new(AtomicUsize::new(0)))
            .collect::<Vec<_>>();

        let reserved_index = (0 .. partition_modes.len())
            .map(|_| CachePadded::new(AtomicUsize::new(0)))
            .collect::<Vec<_>>();

        let wakers = (0 .. partition_modes.len())
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
            partition_modes
        }
    }

    #[inline]
    pub(crate) unsafe fn committed_index(&self, partition_id: usize) -> NonNull<CachePadded<AtomicUsize>> {
        self.committed_index_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) unsafe fn reserved_index(&self, partition_id: usize) -> NonNull<CachePadded<AtomicUsize>> {
        self.reserved_index_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) unsafe fn boundary_index(&self, partition_id: usize) -> NonNull<CachePadded<AtomicUsize>> {
        let shifted_index = if partition_id == 0 {
            self.partition_modes.len() - 1
        } else {
            partition_id - 1
        };

        self.committed_index_ptr.add(shifted_index)
    }

    #[inline]
    pub(crate) unsafe fn waker(&self, partition_id: usize) -> NonNull<CachePadded<BlockingWaitStrategy>> {
        self.waker_ptr.add(partition_id)
    }

    #[inline]
    pub(crate) fn partition_mode(&self, partition_id: usize) -> TalariaResult<&PartitionMode> {
        self.partition_modes
            .get(partition_id)
            .ok_or(TalariaError::PartitionNotFound {
                partition_id
            })
    }
}

impl Drop for PartitionState2 {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.committed_index_ptr.as_ptr(), self.partition_modes.len(), self.partition_modes.len());
            Vec::from_raw_parts(self.reserved_index_ptr.as_ptr(), self.partition_modes.len(), self.partition_modes.len());
            Vec::from_raw_parts(self.waker_ptr.as_ptr(), self.partition_modes.len(), self.partition_modes.len());
        }
    }
}

#[derive(Debug)]
pub struct PartitionState {
    pub(crate) inner: NonNull<PartitionStateInner>,
    pub(crate) boundary_state: NonNull<PartitionStateInner>,
}

impl PartitionState {
    pub(crate) fn new(partition_id: usize, mode: PartitionMode) -> Self {
        let inner = Box::new(PartitionStateInner::new(partition_id, mode));

        PartitionState {
            inner: NonNull::new(Box::leak(inner)).unwrap(),
            boundary_state: NonNull::dangling(),
        }
    }

    pub(crate) fn inner_ptr(&self) -> NonNull<PartitionStateInner> {
        self.inner
    }

    pub(crate) fn set_boundary_state(&mut self, boundary_inner: NonNull<PartitionStateInner>) {
        self.boundary_state = boundary_inner;
    }

    pub(crate) fn inner(&self) -> &PartitionStateInner {
        unsafe { self.inner.as_ref() }
    }

    pub(crate) fn mode(&self) -> &PartitionMode {
        &self.inner().mode
    }
}

impl Drop for PartitionState {
    fn drop(&mut self) {
        unsafe {
            let _ = Box::from_raw(self.inner.as_ptr());
        }
    }
}

#[cfg(any(test, loom))]
mod test_utils {
    use crate::partition::state::PartitionStateInner;
    use crate::partition::PartitionState;

    impl PartitionState {
        pub fn introspect_read_boundary_index(&self) -> usize {
            unsafe {
                self.boundary_state
                    .as_ref()
                    .committed_index
                    .load(std::sync::atomic::Ordering::SeqCst)
            }
        }

        pub fn introspect_inner(&self) -> &PartitionStateInner {
            self.inner()
        }
    }
}
