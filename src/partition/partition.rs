use std::ptr::NonNull;

use crossbeam_utils::CachePadded;
use crate::channel::ChannelInner;

use crate::error::{TalariaError, TalariaResult};
use crate::partition::mode::{Concurrent, Exclusive, PartitionMode, PartitionModeT};
use crate::partition::PartitionState2;
use crate::partition::reservation::Reservation;
use crate::partition::wait::{BlockingWaitStrategy, Token, WaitStrategy};
use crate::sync_types::hint::spin_loop;
use crate::sync_types::sync::atomic::{AtomicUsize, Ordering};
use crate::sync_types::thread::park;
use crate::sync_types::sync::Arc;

/// Partitions represent a distinct "state" of the data managed by a channel
///
/// Partitions have two modes: [Exclusive](Exclusive) or [Concurrent](Concurrent) which controls
/// whether or not multiple instances of the partition can be held at once.
///
/// Partition handles can be created by fetching the partition using
/// [get_exclusive_partition](crate::channel::Channel::get_exclusive_partition)
/// or [get_concurrent_partition](crate::channel::Channel::get_concurrent_partition).
#[derive(Debug)]
pub struct Partition<M: PartitionModeT, T> {
    ring_ptr: NonNull<T>,
    ring_size: usize,
    partition_id: usize,
    committed_index: NonNull<CachePadded<AtomicUsize>>,
    reserved_index: NonNull<CachePadded<AtomicUsize>>,
    boundary_index: NonNull<CachePadded<AtomicUsize>>,
    waker: NonNull<CachePadded<BlockingWaitStrategy>>,
    cached_boundary_index: usize,
    baseline_partition_size: usize,
    #[allow(dead_code)]
    mode: M,
    token: Token,
    _inner: Arc<ChannelInner<T>>,
}

unsafe impl<M: PartitionModeT, T> Send for Partition<M, T> where Box<T>: Send {}

impl<M: PartitionModeT, T> Partition<M, T> {
    /// Returns true if this partition is the primary partition
    ///
    /// Primary partitions are the first defined partition in the partition ring. If this partition
    /// is the primary partition then it will manage all of the data in the channel when the
    /// channel is first built.
    #[inline]
    pub fn is_primary_partition(&self) -> bool {
        self.partition_id == 0
    }

    #[inline]
    fn reserved_index(&self) -> &CachePadded<AtomicUsize> {
        unsafe { self.reserved_index.as_ref() }
    }

    #[inline]
    fn boundary_index(&self) -> &CachePadded<AtomicUsize> {
        unsafe { self.boundary_index.as_ref() }
    }

    fn boundary_signal(&self) -> &CachePadded<BlockingWaitStrategy> {
        unsafe { self.waker.as_ref() }
    }

    /// Returns the total number of elements available in the associated channel
    ///
    /// This does *not* return the length of the partition, but the total number of all elements in
    /// the associated channel
    #[inline]
    pub fn ring_size(&self) -> usize {
        self.ring_size
    }

    fn estimate_remaining(&mut self) -> usize {
        let reserved_index_val = self.reserved_index().load(Ordering::SeqCst);
        self.cached_boundary_index = self.boundary_index().load(Ordering::SeqCst);

        let estimated = self
            .cached_boundary_index
            .wrapping_sub(reserved_index_val)
            .wrapping_add(self.baseline_partition_size);

        // check if we possibly underflowed this calculation
        if estimated.leading_ones() == 0 {
            estimated
        } else {
            0
        }
    }
}

impl<T> Partition<Exclusive, T> {
    pub(crate) fn new(
        partition_id: usize,
        inner: Arc<ChannelInner<T>>,
        partition_state: &PartitionState2,
    ) -> TalariaResult<Self> {
        let partition_mode = partition_state.partition_mode(partition_id)?;

        match partition_mode {
            PartitionMode::Exclusive { ref in_use } => {
                // secure the exclusive partition by marking it as in-use
                // todo: can this be `Acquire`?
                in_use
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .map_err(|_| TalariaError::ExclusivePartitionInUse { partition_id })?;

                let exclusive = Exclusive {
                    in_use: in_use.clone(),
                };

                let ring_ptr = inner.ring_ptr();
                let ring_size = inner.len();
                let (
                    committed_index,
                    reserved_index,
                    boundary_index,
                    waker
                ) = unsafe {
                    let committed_index = partition_state.committed_index(partition_id);
                    let reserved_index = partition_state.reserved_index(partition_id);
                    let boundary_index = partition_state.boundary_index(partition_id);
                    let waker = partition_state.waker(partition_id);

                    (committed_index, reserved_index, boundary_index, waker)
                };

                let mut partition = Self {
                    partition_id,
                    mode: exclusive,
                    ring_ptr,
                    ring_size,
                    committed_index,
                    reserved_index,
                    boundary_index,
                    waker,
                    cached_boundary_index: 0,
                    baseline_partition_size: 0,
                    token: Token::new(),
                    _inner: inner
                };

                partition.baseline_partition_size = if partition.is_primary_partition() {
                    partition.ring_size()
                } else {
                    0
                };

                // preemptive caching of the boundary index
                partition.cached_boundary_index = partition.boundary_index().load(Ordering::SeqCst);

                Ok(partition)
            }
            _ => Err(TalariaError::PartitionNotExclusive { partition_id }),
        }
    }

    /// Attempts to grab a reservation of the specified size on this partition.
    ///
    /// If the amount is 0, greater than the ring size, or if not enough space is available, an
    /// error is returned.
    pub fn try_reserve(&mut self, amount: usize) -> TalariaResult<Reservation<Exclusive, T>> {
        // 1. check if there is enough space
        if amount == 0 {
            return Err(TalariaError::RequestedZeroElements {
                partition_id: self.partition_id,
            });
        }

        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: self.partition_id,
                requested: amount,
            });
        }

        let reserved_index = get_reserved_index_if_enough_space_available(amount, self)?;

        // 2. attempt to reserve the space
        let new_reserved_index = reserved_index.wrapping_add(amount);
        self.reserved_index()
            .store(new_reserved_index, Ordering::SeqCst);

        // 3. return the reservation
        Ok(Reservation::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            amount,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }

    /// Attempts to grab a reservation of the remaining space on this partition.
    ///
    /// The number of remaining element is estimated and then attempted to be fetched from the
    /// partition. This method provides *no guarantee* that if it succeeds, *all* of the
    /// remaining elements are fetched.
    pub fn try_reserve_remaining(&mut self) -> TalariaResult<Reservation<Exclusive, T>> {
        let estimated = self.estimate_remaining();

        self.try_reserve(estimated)
    }

    /// Acquires a reservation of the requested size on this partition.
    ///
    /// This method is blocking in that it will continue to try acquiring the
    /// reservation until available. It will return an error if the requested amount is 0 or greater
    /// than the ring size.
    pub fn reserve(&mut self, amount: usize) -> TalariaResult<Reservation<Exclusive, T>> {
        // 1. check if there is enough space
        if amount == 0 {
            return Err(TalariaError::RequestedZeroElements {
                partition_id: self.partition_id,
            });
        }

        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: self.partition_id,
                requested: amount,
            });
        }

        let AvailableReservation {
            reserved_index, ..
        } = get_reserved_index_when_requested_space_available(
            amount,
            self,
            ReservationIndexStrategy::Lazy,
        )?;

        // 2. attempt to reserve the space
        let new_reserved_index = reserved_index.wrapping_add(amount);
        self.reserved_index()
            .store(new_reserved_index, Ordering::Release);

        // 3. return the reservation
        Ok(Reservation::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            amount,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }

    /// Acquires a reservation of the remaining space on this partition.
    ///
    /// This method blocks until at least 1 element is available to be reserved, then returns
    /// a reservation of all available elements.
    pub fn reserve_remaining(&mut self) -> TalariaResult<Reservation<Exclusive, T>> {
        // wait until we get a reservation with _at least_ 1 element in it
        let AvailableReservation {
            reserved_index,
            available,
        } = get_reserved_index_when_requested_space_available(
            1,
            self,
            ReservationIndexStrategy::Lazy,
        )?;

        // 2. attempt to reserve the space
        let new_reserved_index = reserved_index.wrapping_add(available);
        self.reserved_index()
            .store(new_reserved_index, Ordering::Release);

        // 3. return the reservation
        Ok(Reservation::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            available,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }
}

impl<T> Partition<Concurrent, T> {
    pub(crate) fn new(
        partition_id: usize,
        inner: Arc<ChannelInner<T>>,
        partition_state: &PartitionState2,
    ) -> TalariaResult<Self> {
        let partition_mode = partition_state.partition_mode(partition_id)?;

        match partition_mode {
            PartitionMode::Concurrent => {
                let ring_ptr = inner.ring_ptr();
                let ring_size = inner.len();

                let (
                    committed_index,
                    reserved_index,
                    boundary_index,
                    waker
                ) = unsafe {
                    let committed_index = partition_state.committed_index(partition_id);
                    let reserved_index = partition_state.reserved_index(partition_id);
                    let boundary_index = partition_state.boundary_index(partition_id);
                    let waker = partition_state.waker(partition_id);

                    (committed_index, reserved_index, boundary_index, waker)
                };

                let mut partition = Self {
                    partition_id,
                    mode: Concurrent,
                    ring_ptr,
                    ring_size,
                    committed_index,
                    reserved_index,
                    boundary_index,
                    waker,
                    cached_boundary_index: 0,
                    baseline_partition_size: 0,
                    token: Token::new(),
                    _inner: inner,
                };

                partition.baseline_partition_size = if partition.is_primary_partition() {
                    partition.ring_size()
                } else {
                    0
                };

                partition.cached_boundary_index = partition.boundary_index().load(Ordering::SeqCst);

                Ok(partition)
            }
            _ => Err(TalariaError::PartitionNotConcurrent { partition_id }),
        }
    }

    /// Attempts to grab a reservation of the specified size on this partition.
    ///
    /// If the amount is 0, greater than the ring size, or if not enough space is available, an
    /// error is returned. This method can also fail spuriously if another thread has already
    /// reserved space on this partition in the middle of the reservation process.
    pub fn try_reserve(&mut self, amount: usize) -> TalariaResult<Reservation<Concurrent, T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::NotEnoughSpace {
                partition_id: self.partition_id,
                requested: amount,
                available: self.ring_size(),
            });
        }

        let reserved_index_val = get_reserved_index_if_enough_space_available(amount, self)?;

        // 2. reserve the space
        let new_reserved_index = reserved_index_val.wrapping_add(amount);
        let reservation_attempt = self.reserved_index().compare_exchange(
            reserved_index_val,
            new_reserved_index,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );

        if reservation_attempt.is_err() {
            // inform the caller that a reservation is already out on an exclusive partition
            return Err(TalariaError::SpuriousReservationFailure {
                partition_id: self.partition_id,
                requested: amount,
            });
        }

        // 3. return the reservation
        Ok(Reservation::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index_val,
            new_reserved_index,
            amount,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }

    /// Attempts to grab a reservation of the remaining space on this partition.
    ///
    /// The number of remaining element is estimated and then attempted to be fetched from the
    /// partition. This method provides *no guarantee* that if it succeeds, *all* of the
    /// remaining elements are fetched.
    pub fn try_reserve_remaining(&mut self) -> TalariaResult<Reservation<Concurrent, T>> {
        let estimated = self.estimate_remaining();
        self.try_reserve(estimated)
    }

    /// Acquires a reservation of the requested size on this partition.
    ///
    /// This method is blocking in that it will continue to try acquiring the
    /// reservation until available.
    pub fn reserve(&mut self, amount: usize) -> TalariaResult<Reservation<Concurrent, T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: self.partition_id,
                requested: amount,
            });
        }

        let (reserved_index, new_reserved_index) = loop {
            let AvailableReservation {
                reserved_index, ..
            } = get_reserved_index_when_requested_space_available(
                amount,
                self,
                ReservationIndexStrategy::Pessimistic,
            )?;

            // 2. attempt to reserve the space
            // todo: could this be a weaker order?
            let new_reserved_index = reserved_index.wrapping_add(amount);
            let reservation_result = self.reserved_index().compare_exchange(
                reserved_index,
                new_reserved_index,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );

            if reservation_result.is_ok() {
                break (reserved_index, new_reserved_index);
            }

            #[cfg(loom)]
            {
                loom::thread::yield_now();
                loom::skip_branch();
            }
        };

        // 3. return the reservation
        Ok(Reservation::<'_>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            amount,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }

    /// Acquires a reservation of the remaining space on this partition.
    ///
    /// This method blocks until at least 1 element is available to be reserved, then returns
    /// a reservation of all available elements.
    pub fn reserve_remaining(&mut self) -> TalariaResult<Reservation<Concurrent, T>> {
        let (reserved_index, new_reserved_index, available) = loop {
            let AvailableReservation {
                reserved_index,
                available,
            } = get_reserved_index_when_requested_space_available(
                1,
                self,
                ReservationIndexStrategy::Pessimistic,
            )?;

            // 2. attempt to reserve the space
            let new_reserved_index = reserved_index.wrapping_add(available);
            // todo: can this be `Release`? (probably should be)
            let reservation_result = self.reserved_index().compare_exchange(
                reserved_index,
                new_reserved_index,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );

            if reservation_result.is_ok() {
                break (reserved_index, new_reserved_index, available);
            }
        };

        // 3. return the reservation
        Ok(Reservation::<Concurrent, T>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            available,
            self.committed_index,
            self.reserved_index,
            self.waker
        ))
    }
}

impl<T> Clone for Partition<Concurrent, T> {
    fn clone(&self) -> Self {
        Partition {
            ring_ptr: self.ring_ptr,
            ring_size: self.ring_size,
            partition_id: self.partition_id,
            committed_index: self.committed_index,
            reserved_index: self.reserved_index,
            boundary_index: self.boundary_index,
            waker: self.waker,
            cached_boundary_index: self.cached_boundary_index,
            baseline_partition_size: self.baseline_partition_size,
            mode: Concurrent,
            token: Token::new(),
            _inner: self._inner.clone(),
        }
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        // mark this partition free to be requested again

        // todo: can this be `Release`?
        self.in_use.store(false, Ordering::SeqCst)
    }
}

/// checks if the partition has enough room for the requested amount of space,
/// returning the initial index if so
///
/// this function will also refresh the cached boundary index if not enough
/// space was determined to be available
#[inline]
fn get_reserved_index_if_enough_space_available<M: PartitionModeT, T>(
    requested: usize,
    partition: &mut Partition<M, T>,
) -> TalariaResult<usize> {
    let mut has_refreshed_boundary_index = false;

    let reserved_index_val = partition
        .reserved_index()
        .load(Ordering::Acquire);

    loop {
        let available_elements = partition
            .cached_boundary_index
            .wrapping_sub(reserved_index_val)
            .wrapping_add(partition.baseline_partition_size);

        /*
           this check allows us to determine if our `cached_boundary_index` was so out of date that
           our partition will report "available space" despite that not being true.

           this can occur if the `cached_boundary_index` is out of date and the `start_index_val` is
           larger, leading to an underflow that results in a large number.

           this condition _can_ happen intentionally, though, on primary partitions which we then expect
           to *overflow* the integer back into the low end of `[0->)` by adding the partition data length.
        */
        let did_underflow_occur = available_elements.leading_ones() > 0;

        match available_elements {
            // if enough space is available, return the start index
            _ if available_elements >= requested && !did_underflow_occur => {
                return Ok(reserved_index_val)
            }
            // if not enough space was available, but we've never fetched the boundary index
            _ if !has_refreshed_boundary_index => {
                has_refreshed_boundary_index = true;
                partition.cached_boundary_index = partition.boundary_index().load(Ordering::Acquire)
            }
            // if we've already refreshed the boundary and space was still not available, return an error
            _ => {
                return Err(TalariaError::NotEnoughSpace {
                    partition_id: partition.partition_id,
                    requested,
                    available: available_elements,
                })
            }
        }
    }
}

struct AvailableReservation {
    reserved_index: usize,
    available: usize,
}

#[inline]
fn get_reserved_index_when_requested_space_available<M: PartitionModeT, T>(
    requested: usize,
    partition: &mut Partition<M, T>,
    reservation_strategy: ReservationIndexStrategy,
) -> TalariaResult<AvailableReservation> {
    const MAX_SPIN_LOOP: u32 = 1 << 12;
    const DEFAULT_SPINS: u32 = 1 << 0;

    let mut spins: u32 = DEFAULT_SPINS;
    let mut registered = false;

    let mut reserved_index_val = partition
        .reserved_index()
        .load(Ordering::Acquire);

    loop {
        let available_elements = partition
            .cached_boundary_index
            .wrapping_sub(reserved_index_val)
            .wrapping_add(partition.baseline_partition_size);

        let did_underflow_occur = available_elements.leading_ones() > 0;

        match available_elements {
            // if enough space is available, return the start index
            _ if available_elements >= requested && !did_underflow_occur => {
                if registered {
                    partition.boundary_signal().unregister(&partition.token);
                }

                return Ok(AvailableReservation {
                    reserved_index: reserved_index_val,
                    available: available_elements,
                });
            }
            _ if registered => {
                // park the thread since we know we're not going to get space any time soon
                park();

                #[cfg(loom)]
                {
                    loom::hint::spin_loop();
                    loom::thread::yield_now();
                    loom::skip_branch();
                }
                // reset our refreshes
                registered = false;

                // update our cached boundary since this value updated
                partition.cached_boundary_index =
                    partition.boundary_index().load(Ordering::Acquire);
            }
            // if not enough space was available, but we've never fetched the boundary index
            _ if spins < MAX_SPIN_LOOP => {
                for _ in 0..spins {
                    spin_loop();
                }

                #[cfg(loom)]
                loom::thread::yield_now();

                spins <<= 1;

                partition.cached_boundary_index =
                    partition.boundary_index().load(Ordering::Acquire);
            }
            // if enough space is not available
            // and we already spun for a while
            // we should register for a notification, check the condition one last time, and then
            // park the thread
            _ => {
                // reset our refreshes
                spins = DEFAULT_SPINS;

                partition.boundary_signal().register(&partition.token);

                // check condition one last time
                partition.cached_boundary_index =
                    partition.boundary_index().load(Ordering::Acquire);
            }
        }

        if let ReservationIndexStrategy::Pessimistic = reservation_strategy {
            // to be safe, we should update our reserved index as well
            reserved_index_val = partition
                .reserved_index()
                .load(Ordering::Acquire);
        }
    }
}

enum ReservationIndexStrategy {
    Pessimistic,
    Lazy,
}
