use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::NonNull;
use crate::partition::state::PartitionStateInner;
use crate::partition::wait::WaitStrategy;

pub struct Reservation<'p, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    partition_state: NonNull<PartitionStateInner>,
    inner: PhantomData<&'p ()>
}

impl<T> Reservation<'_, T> {
    pub(crate) fn new<'p>(
        ring_ptr: NonNull<T>,
        ring_size: usize,
        start_index: usize,
        end_index: usize,
        partition_state: NonNull<PartitionStateInner>,
    ) -> Self {
        Self {
            ring_ptr,
            ring_size,
            start_index,
            end_index,
            partition_state,
            inner: PhantomData
        }
    }

    pub fn len(&self) -> usize {
        self.end_index.wrapping_sub(self.start_index)
    }

    pub fn is_empty(&self) -> bool {
        self.start_index == self.end_index
    }

    fn partition_state(&self) -> &PartitionStateInner {
        unsafe { self.partition_state.as_ref() }
    }

    pub fn iter(&self) -> Iter<T> {
        Iter {
            ring_ptr: self.ring_ptr,
            ring_size: self.ring_size,
            start_index: self.start_index,
            end_index: self.end_index,
            offset: 0,
            inner_phantom: PhantomData
        }
    }
}

impl<T> Drop for Reservation<'_, T> {
    fn drop(&mut self) {
        const SPIN: usize = 16;
        // if the reservation is empty, do nothing
        if self.is_empty() {
            return;
        }

        // todo implement a less expensive spinlock here
        // wait for our turn to move the committed index forward
        while self.partition_state()
                .committed_index()
                .load(std::sync::atomic::Ordering::Acquire) != self.start_index {
            for _ in 0 .. SPIN {
                std::hint::spin_loop();
            }
            std::thread::yield_now(); // todo: should we remove this? should we get `wait` here and park?
        }

        // todo: Switching from SeqCst to Release ordering here results in a massive performance improvement. test that this is safe
        // it's our turn now, so let's commit the reservation
        let end = self.end_index;
        let p_state = self.partition_state();
        let c_index = p_state.committed_index();
        c_index.store(end, std::sync::atomic::Ordering::Release);

        // notify all observers that we updated the committed index
        // todo: determine if there is a better way, since this is expensive on drop
        self.partition_state()
            .waker
            .notify();
    }
}

pub struct Iter<'r, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    offset: usize,
    inner_phantom: PhantomData<&'r ()>
}

impl<T> Index<usize> for Reservation<'_, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index >= self.len() {
            panic!("index out of bounds");
        }

        let virtual_address = self.get_virtual_address(index);
        let physical_address = self.get_physical_address(virtual_address);

        unsafe { self.ring_ptr.add(physical_address).as_ref() }
    }
}

impl<T> IndexMut<usize> for Reservation<'_, T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.len() {
            panic!("index out of bounds");
        }

        let virtual_address = self.get_virtual_address(index);
        let physical_address = self.get_physical_address(virtual_address);

        unsafe { self.ring_ptr.add(physical_address).as_mut() }
    }
}

impl<'r, T: 'r> Iterator for Iter<'r, T> {
    type Item = &'r T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.len() {
            return None;
        }

        let virtual_address = self.get_virtual_address(self.offset);
        let physical_address = self.get_physical_address(virtual_address);

        self.offset = self.offset.wrapping_add(1);

        let element = unsafe { self.ring_ptr.add(physical_address).as_ref() };

        Some(element)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining_length = self.len().wrapping_sub(self.offset);

        (remaining_length, Some(remaining_length))
    }
}

trait ProjectedIndexing {
    fn len(&self) -> usize;
    /// Maps an offset `[0, usize::MAX]` to a virtual address `[self.start_index, usize::MAX]`
    fn get_virtual_address(&self, offset: usize) -> usize;
    /// Maps a virtual address `[0, usize::MAX]` to a physical address `[0, self.data.len())`
    fn get_physical_address(&self, virtual_address: usize) -> usize;
}

impl<T> ProjectedIndexing for Reservation<'_, T> {
    fn len(&self) -> usize {
        self.end_index.wrapping_sub(self.start_index)
    }

    fn get_virtual_address(&self, offset: usize) -> usize {
        self.start_index.wrapping_add(offset)
    }

    fn get_physical_address(&self, virtual_address: usize) -> usize {
        virtual_address & (self.ring_size - 1)
    }
}

impl<T> ProjectedIndexing for Iter<'_, T> {
    fn len(&self) -> usize {
        self.end_index.wrapping_sub(self.start_index)
    }

    fn get_virtual_address(&self, offset: usize) -> usize {
        self.start_index.wrapping_add(offset)
    }

    fn get_physical_address(&self, virtual_address: usize) -> usize {
        virtual_address & (self.ring_size - 1)
    }
}