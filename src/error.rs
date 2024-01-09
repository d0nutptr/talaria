use thiserror::Error;

pub type TalariaResult<T> = Result<T, TalariaError>;

#[derive(Debug, Error)]
pub enum TalariaError {
    #[error("too few partitions requested on partitioned ring. requested: {requested}. required: 2")]
    TooFewPartitions {
        requested: usize
    },
    #[error("number of elements in partitioned ring must be a power of two. presented: {count}")]
    ElementsNotPowerOfTwo {
        count: usize
    },
    #[error("partition not found. requested: {partition_id}")]
    PartitionNotFound {
        partition_id: usize,
    },
    #[error("partition is not exclusive. requested: {partition_id}")]
    PartitionNotExclusive {
        partition_id: usize,
    },
    #[error("partition is not concurrent. requested: {partition_id}")]
    PartitionNotConcurrent {
        partition_id: usize,
    },
    #[error("this exclusive partition is already in use. requested: {partition_id}")]
    ExclusivePartitionInUse {
        partition_id: usize,
    },
    #[error("this exclusive partition has an active reservation. partition: {partition_id}")]
    ExclusivePartitionHasActiveReservation {
        partition_id: usize,
    },
    #[error("not enough space in partition. requested: {requested}, available: {available}")]
    NotEnoughSpace {
        partition_id: usize,
        requested: usize,
        available: usize,
    },
    #[error("requested more elements than held in ring. requested: {requested}")]
    RequestedMoreThanPossible {
        partition_id: usize,
        requested: usize,
    },
    #[error("a spurious failure occurred attempting to acquire a reservation on a concurrent partition. please try again. partition id: {partition_id}, requested: {requested}")]
    SpuriousReservationFailure {
        partition_id: usize,
        requested: usize,
    },
    #[error("no elements in ring")]
    NoElements,
    #[error("must request at least 1 element from partition. partition: {partition_id}")]
    RequestedZeroElements {
        partition_id: usize,
    },
    #[error("request to retain reservation too large. reservation size: {reservation_size}, requested retain: {retain_amount}")]
    ReservationRetainTooLarge {
        retain_amount: usize,
        reservation_size: usize,
    }
}