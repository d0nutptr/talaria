use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use crossbeam_utils::CachePadded;

use crate::error::TalariaResult;
use crate::partition::mode::PartitionModeT;
use crate::partition::wait::{BlockingWaitStrategy, WaitStrategy};
use crate::partition::Exclusive;
use crate::sync_types::hint::spin_loop;

/// Represents exclusive ownership to a section of data in a partition.
///
/// If you have a reservation, all of the data inside of the reservation are safe to read from and
/// write to. Reservations are created with partition's various reservation methods.
pub struct Reservation<'p, M: PartitionModeT, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    committed_index: NonNull<CachePadded<AtomicUsize>>,
    reserved_index: NonNull<CachePadded<AtomicUsize>>,
    waker: NonNull<CachePadded<BlockingWaitStrategy>>,
    len: usize,
    inner: PhantomData<&'p M>,
}

impl<'p, M: PartitionModeT + 'p, T> Reservation<'p, M, T> {
    pub(crate) fn new(
        ring_ptr: NonNull<T>,
        ring_size: usize,
        start_index: usize,
        end_index: usize,
        len: usize,
        committed_index: NonNull<CachePadded<AtomicUsize>>,
        reserved_index: NonNull<CachePadded<AtomicUsize>>,
        waker: NonNull<CachePadded<BlockingWaitStrategy>>,
    ) -> Reservation<'p, M, T> {
        Self {
            ring_ptr,
            ring_size,
            start_index,
            end_index,
            committed_index,
            reserved_index,
            waker,
            len,
            inner: PhantomData,
        }
    }
}

impl<M: PartitionModeT, T> Reservation<'_, M, T> {
    /// Returns the number of elements in the reservation.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the reservation contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn committed_index(&self) -> &CachePadded<AtomicUsize> {
        unsafe { self.committed_index.as_ref() }
    }

    fn reserved_index(&self) -> &CachePadded<AtomicUsize> {
        unsafe { self.reserved_index.as_ref() }
    }

    fn waker(&self) -> &CachePadded<BlockingWaitStrategy> {
        unsafe { self.waker.as_ref() }
    }

    /// Creates a read-only iterator over the reservation.
    pub fn iter(&self) -> Iter<T> {
        Iter {
            ring_ptr: self.ring_ptr,
            ring_size: self.ring_size,
            start_index: self.start_index,
            end_index: self.end_index,
            reservation_size: self.len,
            offset: self.start_index,
            inner_phantom: PhantomData,
        }
    }

    /// Creates a mutable iterator over the reservation.
    pub fn iter_mut(&mut self) -> IterMut<T> {
        IterMut {
            ring_ptr: self.ring_ptr,
            ring_size: self.ring_size,
            start_index: self.start_index,
            end_index: self.end_index,
            reservation_size: self.len,
            offset: self.start_index,
            inner_phantom: PhantomData,
        }
    }
}

impl<T> Reservation<'_, Exclusive, T> {
    /// Reduces the number of elements reserved by the reservation.
    ///
    /// Keep in mind that even after a reservation is reduced, the elements that were written to
    /// (but removed) will still hold the new changes. This means that it's important not to
    /// change data if it's important to keep the data in an unaltered state until needed.
    pub fn retain(&mut self, new_size: usize) -> TalariaResult<()> {
        if new_size > self.len {
            return Err(crate::error::TalariaError::ReservationRetainTooLarge {
                retain_amount: new_size,
                reservation_size: self.len,
            });
        }

        self.end_index = self.start_index.wrapping_add(new_size);
        self.len = new_size;

        // normally, this would be dangerous to set reserved index immediately
        // because if the reserved index and committed index match, then an exclusive reservation
        // can be allocated however, as long as *this* reservation exists, another exclusive
        // partition cannot be created because getting an exclusive reservation requires
        // `&mut` and only 1 partition handle can exist at a time. therefore, we can update
        // this immediately
        self.reserved_index()
            .store(self.end_index, std::sync::atomic::Ordering::SeqCst);

        Ok(())
    }
}

impl<M: PartitionModeT, T> Drop for Reservation<'_, M, T> {
    fn drop(&mut self) {
        // if the reservation is empty, do nothing
        if self.is_empty() {
            return;
        }

        // todo implement a less expensive spinlock here
        // wait for our turn to move the committed index forward
        while self
            .committed_index()
            .load(std::sync::atomic::Ordering::Acquire)
            != self.start_index
        {
            const SPIN: usize = 16;

            for _ in 0..SPIN {
                spin_loop();
            }

            #[cfg(loom)]
            loom::thread::yield_now();

            // todo: we should probably consider doing something special if this
            // is a concurrent reservation so we can make sure not to pound the
            // CPU too hard std::thread::yield_now();
            // todo: should we remove this? should we get `wait` here and park?
        }

        // todo: Switching from SeqCst to Release ordering here results in a massive
        // performance improvement. test that this is safe it's our turn now, so
        // let's commit the reservation
        self.committed_index()
            .store(self.end_index, std::sync::atomic::Ordering::Release);

        // notify all observers that we updated the committed index
        // todo: determine if there is a better way, since this is expensive on drop
        self.waker().notify();
    }
}

/// A read-only iterator created by [iter()](Reservation::iter).
pub struct Iter<'r, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    reservation_size: usize,
    offset: usize,
    inner_phantom: PhantomData<&'r ()>,
}

impl<M: PartitionModeT, T> Index<usize> for Reservation<'_, M, T> {
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

impl<M: PartitionModeT, T> IndexMut<usize> for Reservation<'_, M, T> {
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
    /// Maps an offset `[0, usize::MAX]` to a virtual address
    /// `[self.start_index, usize::MAX]`
    fn get_virtual_address(&self, offset: usize) -> usize;

    /// Maps a virtual address `[0, usize::MAX]` to a physical address `[0,
    /// self.data.len())`
    fn get_physical_address(&self, virtual_address: usize) -> usize;
}

impl<M: PartitionModeT, T> ProjectedIndexing for Reservation<'_, M, T> {
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

/// A mutable iterator created by [iter_mut()](Reservation::iter_mut).
pub struct IterMut<'r, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    start_index: usize,
    end_index: usize,
    reservation_size: usize,
    offset: usize,
    inner_phantom: PhantomData<&'r ()>,
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
