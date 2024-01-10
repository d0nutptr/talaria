use std::ptr::NonNull;
use crossbeam_utils::CachePadded;
use crate::sync_types::sync::atomic::AtomicUsize;
use crate::partition::mode::PartitionMode;
use crate::partition::wait::BlockingWaitStrategy;

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
            mode
        }
    }

    pub(crate) fn committed_index(&self) -> &CachePadded<AtomicUsize> {
        &self.committed_index
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
    use crate::partition::PartitionState;
    use crate::partition::state::PartitionStateInner;

    impl PartitionState {
        pub fn introspect_read_boundary_index(&self) -> usize {
            unsafe {
                self.boundary_state.as_ref().committed_index.load(std::sync::atomic::Ordering::SeqCst)
            }
        }

        pub fn introspect_inner(&self) -> &PartitionStateInner {
            self.inner()
        }
    }
}