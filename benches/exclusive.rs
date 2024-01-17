mod common;

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;
use talaria::partition::{Exclusive, Partition};

use crate::common::{bench_scenarios, BenchArgs, DEFAULT_OBJECT_SIZE};

fn run_exclusive_benchmark<M: Default>(
    mut partition: Partition<Exclusive, M>,
    chunk: usize,
    amount: usize,
) {
    for _ in 0..amount {
        let mut reservation = partition.reserve(1).unwrap();
        // black_box(&reservation[0]);
        reservation[0] = M::default();

        // let mut reservation = partition.reserve(chunk).unwrap();
        // for msg in reservation.iter_mut() {
        //     black_box(&msg);
        //     *msg = M::default();
        // }
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

            let objects = vec![[0u8; DEFAULT_OBJECT_SIZE]; channel_size];

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

            // let iterations = 1_000_000_000;
            let other_thread = std::thread::spawn(move || {
                run_exclusive_benchmark(thread_partition, chunk_size, iterations as usize)
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
        BenchArgs::new(1024, 1),
        // BenchArgs::new(1024, 64),
        // BenchArgs::new(4096, 1),
        // BenchArgs::new(4096, 64),
    ]);
}

criterion_group! {
    name = exclusive_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(exclusive_channel);
