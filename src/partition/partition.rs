use std::marker::PhantomData;
use std::ptr::NonNull;
use crossbeam_utils::CachePadded;
use crate::loom::sync::atomic::{AtomicUsize, Ordering};
use crate::loom::thread::park;
use crate::error::{TalariaError, TalariaResult};
use crate::partition::mode::{Concurrent, Exclusive, PartitionMode, PartitionModeT};
use crate::partition::reservation::Reservation;
use crate::partition::state::{PartitionState, PartitionStateInner};
use crate::partition::wait::{BlockingWaitStrategy, Token, WaitStrategy};

#[derive(Debug)]
pub struct Partition<'c, M: PartitionModeT, T> {
    partition_state: NonNull<PartitionStateInner>,
    boundary_state: NonNull<PartitionStateInner>,
    ring_ptr: NonNull<T>,
    ring_size: usize,
    cached_boundary_index: usize,
    #[allow(dead_code)]
    mode: M,
    token: Token,
    inner: PhantomData<&'c ()>
}

impl<'c, M: PartitionModeT, T> Partition<'c, M, T> {
    #[inline]
    pub fn is_primary_partition(&self) -> bool {
        *self.partition_state().partition_id == 0
    }

    #[inline]
    pub(crate) fn partition_state(&self) -> &PartitionStateInner {
        unsafe { self.partition_state.as_ref() }
    }

    #[inline]
    fn boundary_state(&self) -> &PartitionStateInner {
        unsafe { self.boundary_state.as_ref() }
    }

    #[inline]
    fn boundary_index(&self) -> &CachePadded<AtomicUsize> {
        &self.boundary_state().committed_index
    }

    #[inline]
    fn boundary_signal(&self) -> &CachePadded<BlockingWaitStrategy> {
        &self.boundary_state().waker
    }

    #[inline]
    pub fn ring_size(&self) -> usize {
        self.ring_size
    }
}

impl<'c, T> Partition<'c, Exclusive, T> {
    pub(crate) fn new(
        ring_ptr: NonNull<T>,
        ring_size: usize,
        partition_state: &PartitionState
    ) -> TalariaResult<Self> {
        match partition_state.mode() {
            PartitionMode::Exclusive { ref in_use } => {
                // secure the exclusive partition by marking it as in-use
                // todo: can this be `Acquire`?
                in_use
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .map_err(|_| TalariaError::ExclusivePartitionInUse { partition_id: *partition_state.inner().partition_id })?;

                let exclusive = Exclusive { in_use: in_use.clone() };
                let mut partition = Self {
                    mode: exclusive,
                    ring_ptr,
                    ring_size,
                    partition_state: partition_state.inner,
                    boundary_state: partition_state.boundary_state,
                    cached_boundary_index: 0,
                    token: Token::new(),
                    inner: PhantomData
                };

                // preemptive caching of the boundary index
                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst);

                Ok(partition)
            },
            _ => Err(TalariaError::PartitionNotExclusive { partition_id: *partition_state.inner().partition_id })
        }
    }

    pub fn try_reserve(&mut self, amount: usize) -> TalariaResult<Reservation<T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: *self.partition_state().partition_id,
                requested: amount
            });
        }

        let reserved_index = get_reserved_index_if_enough_space_available(
            amount,
            self
        )?;

        // 2. attempt to reserve the space
        let new_reserved_index = reserved_index.wrapping_add(amount);
        self
            .partition_state()
            .reserved_index
            .store(new_reserved_index, Ordering::SeqCst);

        // 3. return the reservation
        Ok(Reservation::<T>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            self.partition_state
        ))
    }

    /// Acquires a reservation of the requested size on this partition.
    ///
    /// This method is blocking in that it will continue to try acquiring the reservation until available
    pub fn reserve(&mut self, amount: usize) -> TalariaResult<Reservation<T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: *self.partition_state().partition_id,
                requested: amount
            });
        }

        let reserved_index = get_reserved_index_when_enough_space_available(
            amount,
            self
        )?;

        // 2. attempt to reserve the space
        let new_reserved_index = reserved_index.wrapping_add(amount);
        self
            .partition_state()
            .reserved_index
            .store(new_reserved_index, Ordering::Release);

        // 3. return the reservation
        Ok(Reservation::<T>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            self.partition_state
        ))
    }
}

impl<'c, T> Partition<'c, Concurrent, T> {
    pub(crate) fn new(ptr: NonNull<T>, len: usize, state: &PartitionState) -> TalariaResult<Self> {
        match state.mode() {
            PartitionMode::Concurrent => {
                let mut partition = Self {
                    mode: Concurrent,
                    ring_ptr: ptr,
                    ring_size: len,
                    partition_state: state.inner,
                    boundary_state: state.boundary_state,
                    cached_boundary_index: 0,
                    token: Token::new(),
                    inner: PhantomData
                };

                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst);

                Ok(partition)
            },
            _ => Err(TalariaError::PartitionNotConcurrent { partition_id: *state.inner().partition_id })
        }
    }

    pub fn try_reserve(&mut self, amount: usize) -> TalariaResult<Reservation<T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::NotEnoughSpace {
                partition_id: *self.partition_state().partition_id,
                requested: amount,
                available: self.ring_size()
            });
        }

        let reserved_index_val = get_reserved_index_if_enough_space_available(amount, self)?;

        // 2. reserve the space
        let new_reserved_index = reserved_index_val.wrapping_add(amount);
        let reservation_attempt = self
            .partition_state()
            .reserved_index
            .compare_exchange(
                reserved_index_val,
                new_reserved_index,
                Ordering::SeqCst,
                Ordering::SeqCst
            );

        if reservation_attempt.is_err() {
            // inform the caller that a reservation is already out on an exclusive partition
            return Err(TalariaError::SpuriousReservationFailure{
                partition_id: *self.partition_state().partition_id,
                requested: amount
            });
        }

        // 3. return the reservation
        Ok(Reservation::<T>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index_val,
            new_reserved_index,
            self.partition_state
        ))
    }

    /// Acquires a reservation of the requested size on this partition.
    ///
    /// This method is blocking in that it will continue to try acquiring the reservation until available
    pub fn reserve(&mut self, amount: usize) -> TalariaResult<Reservation<T>> {
        // 1. check if there is enough space
        if amount > self.ring_size() {
            return Err(TalariaError::RequestedMoreThanPossible {
                partition_id: *self.partition_state().partition_id,
                requested: amount
            });
        }

        let (reserved_index, new_reserved_index) = loop {
            let reserved_index = get_reserved_index_when_enough_space_available(amount, self)?;

            // 2. attempt to reserve the space
            let new_reserved_index = reserved_index.wrapping_add(amount);
            // todo: can this be `Release`? (probably should be)
            let reservation_result = self
                .partition_state()
                .reserved_index
                .compare_exchange(reserved_index, new_reserved_index, Ordering::SeqCst, Ordering::SeqCst);

            if reservation_result.is_ok() {
                break (reserved_index, new_reserved_index)
            }
        };

        // 3. return the reservation
        Ok(Reservation::<T>::new(
            self.ring_ptr,
            self.ring_size,
            reserved_index,
            new_reserved_index,
            self.partition_state
        ))
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        // mark this partition free to be requested again

        // todo: can this be `Release`?
        self.in_use
            .store(false, Ordering::SeqCst)
    }
}

#[inline]
/// checks if the partition has enough room for the requested amount of space, returning the initial index if so
///
/// this function will also refresh the cached boundary index if not enough space was determined to be available
fn get_reserved_index_if_enough_space_available<M: PartitionModeT, T>(
    requested: usize,
    partition: &mut Partition<'_, M, T>,
) -> TalariaResult<usize> {
    let mut has_refreshed_boundary_index = false;

    let reserved_index_val = partition
        .partition_state()
        .reserved_index
        .load(Ordering::SeqCst);

    loop {
        let available_elements = partition
            .cached_boundary_index
            .wrapping_sub(reserved_index_val)
            .wrapping_add(if partition.is_primary_partition() { partition.ring_size() } else { 0 });

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
            _ if available_elements >= requested && !did_underflow_occur => return Ok(reserved_index_val),
            // if not enough space was available, but we've never fetched the boundary index
            _ if !has_refreshed_boundary_index => {
                has_refreshed_boundary_index = true;
                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst)
            },
            // if we've already refreshed the boundary and space was still not available, return an error
            _ => {
                return Err(TalariaError::NotEnoughSpace {
                    partition_id: *partition.partition_state().partition_id,
                    requested,
                    available: available_elements
                })
            }
        }
    }
}

#[inline]
fn get_reserved_index_when_enough_space_available<M: PartitionModeT, T>(
    requested: usize,
    partition: &mut Partition<'_, M, T>,
) -> TalariaResult<usize> {
    const MAX_SPIN_LOOP: u32 = 1 << 12;
    const DEFAULT_SPINS: u32 = 1 << 4;

    let mut spins: u32 = DEFAULT_SPINS;
    let mut registered = false;
    let token = *&partition.token;

    let mut reserved_index_val = partition
        .partition_state()
        .reserved_index
        .load(Ordering::Acquire);

    loop {
        let available_elements = partition
            .cached_boundary_index
            .wrapping_sub(reserved_index_val)
            .wrapping_add(if partition.is_primary_partition() { partition.ring_size() } else { 0 });

        let did_underflow_occur = available_elements.leading_ones() > 0;

        match available_elements {
            // if enough space is available, return the start index
            _ if available_elements >= requested && !did_underflow_occur => {
                if registered {
                    partition
                        .boundary_signal()
                        .unregister(&token);
                }

                return Ok(reserved_index_val)
            },
            _ if registered => {
                // park the thread since we know we're not going to get space any time soon
                park();

                // reset our refreshes
                registered = false;

                // update our cached boundary since this value updated
                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst);

                // to be safe, we should update our reserved index as well
                reserved_index_val = partition
                    .partition_state()
                    .reserved_index
                    .load(Ordering::Acquire);
            }
            // if not enough space was available, but we've never fetched the boundary index
            _ if spins < MAX_SPIN_LOOP => {
                for _ in 0 .. spins {
                    std::hint::spin_loop();
                }

                spins <<= 1;

                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst);
            },
            // if enough space is not available
            // and we already spun for a while
            // we should register for a notification, check the condition one last time, and then park the thread
            _ => {
                // reset our refreshes
                spins = DEFAULT_SPINS;

                partition
                    .boundary_signal()
                    .register(&token);

                // check condition one last time
                partition.cached_boundary_index = partition
                    .boundary_index()
                    .load(Ordering::SeqCst);
            }
        }
    }
}
