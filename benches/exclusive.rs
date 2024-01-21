mod common;

use std::time::Instant;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;
use talaria::partition::{PartitionModeT, PartitionT};

use crate::common::bench_util::{
    bench_scenarios,
    BenchArgs,
    BenchmarkMessageType,
    DEFAULT_OBJECT_SIZE,
};
use crate::common::talaria_util::{talaria_receiver, talaria_sender};

pub fn exclusive_talaria_bench(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let objects = vec![BenchmarkMessageType::default(); channel_size];

            let channel = Channel::builder()
                .add_exclusive_partition()
                .add_exclusive_partition()
                .build(objects)
                .unwrap();

            let sender_partition = channel.get_exclusive_partition(0).unwrap();
            let receiver_partition = channel.get_exclusive_partition(1).unwrap();

            let receiver_thread = std::thread::spawn(move || {
                talaria_receiver(receiver_partition, chunk_size, iterations as usize)
            });

            let start = Instant::now();
            talaria_sender(sender_partition, chunk_size, iterations as usize);
            receiver_thread.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_benchmark(c: &mut Criterion) {
    bench_scenarios(c, "Exclusive Partitions", exclusive_talaria_bench, vec![
        BenchArgs::new(1, 1),
        BenchArgs::new(1024, 1),
        BenchArgs::new(1024, 100),
        BenchArgs::new(4096, 1),
        BenchArgs::new(4096, 100),
    ]);
}

criterion_group! {
    name = exclusive_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(exclusive_channel);
