mod common;

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;

use crate::common::{bench_scenarios, BenchArgs, DEFAULT_OBJECT_SIZE};

fn run_concurrent_benchmark<M: Default>(
    channel: &Channel<M>,
    partition_id: usize,
    chunk: usize,
    amount: usize,
) {
    let mut partition = channel.get_concurrent_partition(partition_id).unwrap();

    for _ in 0..amount {
        let mut reservation = partition.reserve(chunk).unwrap();

        for msg in reservation.iter_mut() {
            black_box(&msg);
            *msg = M::default();
        }
    }
}

pub fn run_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let objects = vec![[0u8; DEFAULT_OBJECT_SIZE]; channel_size];

            let channel = Channel::builder()
                .add_concurrent_partition()
                .add_concurrent_partition()
                .build(objects)
                .unwrap();

            let channel_2 = channel.clone();

            let thread_one = std::thread::spawn(move || {
                run_concurrent_benchmark(&channel, 0, chunk_size, iterations as usize)
            });

            let start = Instant::now();
            run_concurrent_benchmark(&channel_2, 1, chunk_size, iterations as usize);
            thread_one.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_benchmark(c: &mut Criterion) {
    bench_scenarios(c, "Concurrent Partitions", run_benchmark_with_args, vec![
        BenchArgs::new(1024, 1),
        BenchArgs::new(1024, 64),
        BenchArgs::new(4096, 1),
        BenchArgs::new(4096, 64),
    ]);
}

criterion_group! {
    name = concurrent_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(concurrent_channel);
