#![feature(non_null_convenience)]

pub mod channel;
pub mod error;
pub mod partition;
mod loom;

#[test]
fn simple_test_for_miri() {
    use crate::channel::Channel;

    let channel = Channel::<u64>::builder()
        .add_exclusive_partition()
        .add_exclusive_partition()
        .build(vec![0; 4])
        .unwrap();

    let channel2 = channel.clone();

    let handle = std::thread::spawn(move || {
        let mut partition = channel2.get_exclusive_partition(1).unwrap();
        let mut reservation = partition.reserve(1).unwrap();
        reservation[0] = 1;
    });

    let mut partition = channel.get_exclusive_partition(0).unwrap();
    drop(partition.reserve(1));
    handle.join().unwrap();

    for _ in 0 .. 4 {
        let reservation = partition.reserve(1).unwrap();
        std::hint::black_box(reservation);
        // println!("T1: {}", reservation[0]);
    }

    drop(channel);
    println!("done");
}