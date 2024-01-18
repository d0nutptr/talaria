mod common;

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;
use talaria::partition::{Exclusive, Partition};

use crate::common::{bench_scenarios, BenchArgs, DEFAULT_OBJECT_SIZE};

fn run_exclusive_benchmark2(
    mut partition: Partition<Exclusive, i64>,
    chunk: usize,
    amount: usize,
) {
    let mut counter = 0;

    for _ in 0..amount {
        let mut reservation = partition.reserve(chunk).unwrap();

        for msg in reservation.iter() {
            assert_eq!(*msg, counter);
            counter += 1;
        }
    }
}

fn run_exclusive_benchmark(
    mut partition: Partition<Exclusive, i64>,
    chunk: usize,
    amount: usize,
) {
    let mut counter = 0;

    for _ in 0..amount {
        let mut reservation = partition.reserve(chunk).unwrap();

        for msg in reservation.iter_mut() {
            *msg = counter;
            counter += 1;
        }
    }
}

pub fn run_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    const MAIN_PARTITION: usize = 0;
    const THREAD_PARTITION: usize = 1;

    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let mut objects = vec![0i64; channel_size];
            objects.iter_mut().enumerate().for_each(|(idx, elem)| *elem = idx as i64);

            let channel = Channel::builder()
                .add_exclusive_partition()
                .add_exclusive_partition()
                .build(objects)
                .unwrap();

            let main_partition = channel
                .get_exclusive_partition(MAIN_PARTITION)
                .unwrap();
            let thread_partition = channel
                .get_exclusive_partition(THREAD_PARTITION)
                .unwrap();

            let other_thread = std::thread::spawn(move || {
                run_exclusive_benchmark2(thread_partition, chunk_size, iterations as usize)
            });

            let start = Instant::now();
            run_exclusive_benchmark(main_partition, chunk_size, iterations as usize);
            other_thread.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_benchmark(c: &mut Criterion) {
    bench_scenarios(c, "Exclusive Partitions", run_benchmark_with_args, vec![
        // BenchArgs::new(1024, 1),
        // BenchArgs::new(1024, 100),
        // BenchArgs::new(4096, 1),
        // BenchArgs::new(4096, 100),
        BenchArgs::new(65536, 1),
        BenchArgs::new(65536, 100),
    ]);
}

criterion_group! {
    name = exclusive_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(exclusive_channel);
