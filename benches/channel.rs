use std::fmt::{Display, Formatter};
use std::time::Instant;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;

struct BenchArgs<const L: usize> {
    channel_size: usize,
    chunk_size: usize,
}

impl<const L: usize> BenchArgs<L> {
    fn new(channel_size: usize, chunk_size: usize) -> Self {
        Self {
            channel_size,
            chunk_size,
        }
    }
}

impl<const L: usize> Display for BenchArgs<L> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Channel Size: {} elements, Element Size: {} bytes, Reservation Size: {} elements",
            self.channel_size,
            L,
            self.chunk_size,
        ))
    }
}

fn benchmark_exclusive_partition(channel: &Channel<usize>, partition_id: usize, chunk: usize, amount: usize) {
    let mut partition = channel
        .get_exclusive_partition(partition_id)
        .unwrap();

    for _ in 0 .. amount {
        let mut reservation = partition.reserve(chunk).unwrap();

        for elem in reservation.iter() {
            black_box(elem);
            // assert_eq!(reservation[idx], partition_id);
            // reservation[idx] = 1 - partition_id;
        }
    }
}

fn benchmark_concurrent_partition<T>(channel: &Channel<T>, partition_id: usize, chunk: usize, amount: usize) {
    let mut partition = channel
        .get_concurrent_partition(partition_id)
        .unwrap();

    for _ in 0 .. (amount / chunk) {
        black_box(partition.reserve(chunk).unwrap());
    }
}

fn run_two_exclusive_partitions_bench<const L: usize>(c: &mut Criterion, input: BenchArgs<L>) {
    c.bench_with_input(
        BenchmarkId::new("Two Exclusive Partitions", &input),
        &input,
        |b, args | {
            b.iter_custom(|iterations| {
                let BenchArgs {
                    channel_size,
                    chunk_size
                } = args;

                let chunk_size = *chunk_size;
                let objects = [0].repeat(*channel_size);

                let channel = Channel::builder()
                    .add_exclusive_partition()
                    .add_exclusive_partition()
                    .build(objects)
                    .unwrap();

                let channel_2 = channel.clone();

                // let iterations = 100_000_000;
                let thread_one = std::thread::spawn(move || {
                    benchmark_exclusive_partition(&channel, 0, chunk_size, iterations as usize)
                });

                let start = Instant::now();
                benchmark_exclusive_partition(&channel_2, 1, chunk_size, iterations as usize);
                thread_one.join().unwrap();
                start.elapsed()
            });
        }
    );
}

fn run_two_concurrent_partitions_bench<const L: usize>(c: &mut Criterion, input: BenchArgs<L>) {
    c.bench_with_input(
        BenchmarkId::new("Two Concurrent Partitions", &input),
        &input,
        |b, args | {
            b.iter_custom(|iterations| {
                let BenchArgs {
                    channel_size,
                    chunk_size
                } = args;

                let chunk_size = *chunk_size;
                let objects = [[0u8; L]].repeat(*channel_size);

                let channel = Channel::builder()
                    .add_concurrent_partition()
                    .add_concurrent_partition()
                    .build(objects)
                    .unwrap();

                let channel_2 = channel.clone();

                let thread_one = std::thread::spawn(move || {
                    benchmark_concurrent_partition(&channel, 0, 1, iterations as usize)
                });

                let start = Instant::now();
                benchmark_concurrent_partition(&channel_2, 1, chunk_size, iterations as usize);
                thread_one.join().unwrap();
                start.elapsed()
            });
        }
    );
}

fn bench_two_exclusive_partitions(c: &mut Criterion) {
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1>::new(1024, 1));
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1024>::new(1024, 1));
    run_two_exclusive_partitions_bench(c, BenchArgs::<1>::new(1024, 64));
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1024>::new(1024, 8));
    //
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1>::new(4096, 1));
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1024>::new(4096, 1));
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1>::new(4096, 8));
    // run_two_exclusive_partitions_bench(c, BenchArgs::<1024>::new(4096, 8));
}

fn bench_two_concurrent_partitions(c: &mut Criterion) {
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1>::new(1024, 1));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1024>::new(1024, 1));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1>::new(1024, 8));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1024>::new(1024, 8));
    //
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1>::new(4096, 1));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1024>::new(4096, 1));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1>::new(4096, 8));
    // run_two_concurrent_partitions_bench(c, BenchArgs::<1024>::new(4096, 8));
}

criterion_group! {
    name = channel_benches;
    config = Criterion::default().sample_size(500).with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_two_exclusive_partitions, bench_two_concurrent_partitions
}

criterion_main!(channel_benches);