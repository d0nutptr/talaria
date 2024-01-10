use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::NonNull;
use crate::sync_types::hint::spin_loop;
use crate::error::TalariaResult;
use crate::partition::Exclusive;
use crate::partition::mode::PartitionModeT;
use crate::partition::state::PartitionStateInner;
use crate::partition::wait::WaitStrategy;

pub struct Reservation<'p, M, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    len: usize,
    partition_state: NonNull<PartitionStateInner>,
    inner: PhantomData<&'p M>
}

impl<'p, M: PartitionModeT + 'p, T> Reservation<'p, M, T> {
    pub(crate) fn new(
        ring_ptr: NonNull<T>,
        ring_size: usize,
        start_index: usize,
        end_index: usize,
        len: usize,
        partition_state: NonNull<PartitionStateInner>,
    ) -> Reservation<'p, M, T> {
        Self {
            ring_ptr,
            ring_size,
            start_index,
            end_index,
            len,
            partition_state,
            inner: PhantomData
        }
    }
}

impl<M, T> Reservation<'_, M, T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
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
            reservation_size: self.len,
            offset: self.start_index,
            inner_phantom: PhantomData
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<T> {
        IterMut {
            ring_ptr: self.ring_ptr,
            ring_size: self.ring_size,
            start_index: self.start_index,
            end_index: self.end_index,
            reservation_size: self.len,
            offset: self.start_index,
            inner_phantom: PhantomData
        }
    }
}

impl<T> Reservation<'_, Exclusive, T> {
    pub fn retain(&mut self, new_size: usize) -> TalariaResult<()> {
        if new_size > self.len {
            return Err(crate::error::TalariaError::ReservationRetainTooLarge {
                retain_amount: new_size,
                reservation_size: self.len
            });
        }

        self.end_index = self.start_index.wrapping_add(new_size);
        self.len = new_size;

        Ok(())
    }
}

impl<M, T> Drop for Reservation<'_, M, T> {
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
                spin_loop();
            }

            #[cfg(loom)]
            loom::thread::yield_now();

            // todo: we should probably consider doing something special if this is a concurrent reservation so we can make sure not to pound the CPU too hard
            // std::thread::yield_now(); // todo: should we remove this? should we get `wait` here and park?
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
    reservation_size: usize,
    offset: usize,
    inner_phantom: PhantomData<&'r ()>
}

impl<M, T> Index<usize> for Reservation<'_, M, T> {
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

impl<M, T> IndexMut<usize> for Reservation<'_, M, T> {
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
        if self.offset == self.end_index {
            return None;
        }

        let physical_address = self.get_physical_address(self.offset);
        let element = unsafe { self.ring_ptr.add(physical_address).as_ref() };

        self.offset = self.offset.wrapping_add(1);

        Some(element)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining_length = self.end_index.wrapping_sub(self.offset);

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

impl<M, T> ProjectedIndexing for Reservation<'_, M, T> {
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
        self.reservation_size
    }

    fn get_virtual_address(&self, offset: usize) -> usize {
        self.start_index.wrapping_add(offset)
    }

    fn get_physical_address(&self, virtual_address: usize) -> usize {
        virtual_address & (self.ring_size - 1)
    }
}

pub struct IterMut<'r, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    reservation_size: usize,
    offset: usize,
    inner_phantom: PhantomData<&'r ()>
}

impl<T> ProjectedIndexing for IterMut<'_, T> {
    fn len(&self) -> usize {
        self.reservation_size
    }

    fn get_virtual_address(&self, offset: usize) -> usize {
        self.start_index.wrapping_add(offset)
    }

    fn get_physical_address(&self, virtual_address: usize) -> usize {
        virtual_address & (self.ring_size - 1)
    }
}

impl<'r, T: 'r> Iterator for IterMut<'r, T> {
    type Item = &'r mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.end_index {
            return None;
        }

        let physical_address = self.get_physical_address(self.offset);
        let element = unsafe { self.ring_ptr.add(physical_address).as_mut() };

        self.offset = self.offset.wrapping_add(1);

        Some(element)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining_length = self.end_index.wrapping_sub(self.offset);

        (remaining_length, Some(remaining_length))
    }
}

impl<'r, T: 'r> ExactSizeIterator for Iter<'r, T> {}
impl<'r, T: 'r> ExactSizeIterator for IterMut<'r, T> {}