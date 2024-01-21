mod common;

use std::time::Instant;

use common::bench_util::BenchmarkMessageType;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;
use talaria::partition::PartitionT;

use crate::common::bench_util::{bench_scenarios, BenchArgs};
use crate::common::talaria_util::{talaria_receiver, talaria_sender};

pub fn concurrent_uncontended_talaria_bench(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let objects = vec![BenchmarkMessageType::default(); channel_size];

            let channel = Channel::builder()
                .add_concurrent_partition()
                .add_concurrent_partition()
                .build(objects)
                .unwrap();

            let sender_partition = channel.get_concurrent_partition(0).unwrap();
            let receiver_partition = channel.get_concurrent_partition(1).unwrap();

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

pub fn concurrent_contended_talaria_bench(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let objects = vec![BenchmarkMessageType::default(); channel_size];

            let channel = Channel::builder()
                .add_concurrent_partition()
                .add_concurrent_partition()
                .build(objects)
                .unwrap();

            // let iterations = 10_000_000;

            let sender_one_partition = channel.get_concurrent_partition(0).unwrap();
            let sender_two_partition = channel.get_concurrent_partition(0).unwrap();
            let receiver_partition = channel.get_concurrent_partition(1).unwrap();

            let sender_one_thread = std::thread::spawn(move || {
                talaria_sender(sender_one_partition, chunk_size, iterations as usize)
            });
            let sender_two_thread = std::thread::spawn(move || {
                talaria_sender(sender_two_partition, chunk_size, iterations as usize)
            });
            let receiver_one_thread = std::thread::spawn(move || {
                talaria_receiver(receiver_partition, chunk_size, 2 * iterations as usize)
            });

            let start = Instant::now();
            sender_one_thread.join().unwrap();
            sender_two_thread.join().unwrap();
            receiver_one_thread.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_uncontended_benchmark(c: &mut Criterion) {
    // bench_scenarios(c, "Concurrent Partitions (Uncontended)", concurrent_uncontended_talaria_bench, vec![
    //     BenchArgs::new(1, 1),
    //     BenchArgs::new(1024, 1),
    //     BenchArgs::new(1024, 64),
    //     BenchArgs::new(4096, 1),
    //     BenchArgs::new(4096, 64),
    // ]);
}

fn run_contended_benchmark(c: &mut Criterion) {
    bench_scenarios(c, "Concurrent Partitions (Contended)", concurrent_contended_talaria_bench, vec![
        // BenchArgs::new(1, 1),
        BenchArgs::new(1024, 1),
        BenchArgs::new(1024, 64),
        BenchArgs::new(4096, 1),
        // BenchArgs::new(4096, 64),
    ]);
}

criterion_group! {
    name = concurrent_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_uncontended_benchmark, run_contended_benchmark
}

criterion_main!(concurrent_channel);
