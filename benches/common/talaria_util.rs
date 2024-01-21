use std::hint::black_box;

use talaria::partition::{Partition, PartitionModeT, PartitionT};

pub fn talaria_sender<M: PartitionModeT, T: Default>(
    mut partition: Partition<M, T>,
    chunk: usize,
    amount: usize,
) where
    Partition<M, T>: PartitionT<T>,
{
    for _ in 0..amount {
        let mut reservation = partition.reserve(chunk).unwrap();

        for msg in reservation.iter_mut() {
            // to emulate "writing" a message
            *msg = black_box(T::default());
        }
    }
}

pub fn talaria_receiver<M: PartitionModeT, T: Default>(
    mut partition: Partition<M, T>,
    chunk: usize,
    amount: usize,
) where
    Partition<M, T>: PartitionT<T>,
{
    for _ in 0..amount {
        let reservation = partition.reserve(chunk).unwrap();

        for msg in reservation.iter() {
            // to emulate "reading" a message
            black_box(msg);
        }
    }
}
