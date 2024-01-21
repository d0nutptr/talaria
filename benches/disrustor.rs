mod common;

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use disrustor::internal::{
    BatchEventProcessor,
    RingBuffer,
    SingleProducerSequencer,
    SpinLoopWaitStrategy,
    ThreadedExecutor,
};
use disrustor::{
    DataProvider,
    EventHandler,
    EventProcessorExecutor,
    EventProcessorMut,
    ExecutorHandle,
    Sequence,
    Sequencer,
    WaitStrategy,
};
use pprof::criterion::{Output, PProfProfiler};
use talaria::channel::Channel;
use talaria::partition::{Exclusive, Partition, PartitionT};

use crate::common::bench_util::{bench_scenarios, BenchArgs};

struct Checker;

impl EventHandler<i64> for Checker {
    fn handle_event(&mut self, event: &i64, seq: Sequence, _: bool) {
        assert_eq!(*event, seq);
    }
}

// This benchmark was (mostly) copied, from: https://github.com/sklose/disrustor/blob/e6a3e9226aa262fd916bb476b1c9c1d33b36d141/benches/u64_channels.rs#L34
// The main purpose of the changes was to support dynamic channel capacity and clarifying the
// arguments' purposes. instead of 2^16 capacity, using passed in capacity
fn disrustor_channel<S: Sequencer, F: FnOnce(&RingBuffer<i64>) -> S>(
    capacity: usize,
    iterations: u64,
    chunk_size: u64,
    f: F,
) {
    let data: Arc<RingBuffer<i64>> = Arc::new(RingBuffer::new(capacity));
    let mut sequencer = f(data.as_ref());

    let gating_sequence = vec![sequencer.get_cursor()];
    let barrier = sequencer.create_barrier(&gating_sequence);
    let processor = BatchEventProcessor::create(Checker {});

    sequencer.add_gating_sequence(&processor.get_cursor());

    let executor = ThreadedExecutor::with_runnables(vec![processor.prepare(barrier, data.clone())]);

    let handle = executor.spawn();

    let mut counter = 0;
    for _ in 1..=iterations {
        let mut remainder = chunk_size as i64;
        while remainder > 0 {
            let (start, end) = sequencer.next(remainder as usize);
            let count = end - start + 1;
            remainder -= count;
            for sequence in start..=end {
                counter += 1;
                unsafe { *data.get_mut(sequence) = sequence };
            }
            sequencer.publish(start, end);
        }
    }

    sequencer.drain();
    handle.join();
    assert_eq!(counter, iterations * chunk_size);
}

/// This benchmark is a bit special
///
/// Disrustor is the closest to Talaria in terms of performance. As such, to give Disrustor the
/// "fairest" fight, we want to use their official benchmark (which presumably show Disrustor in the
/// best light). This benchmark pits Disrustor's official benchmark (with only minor modifications
/// to allow for multiple channels sizes) against Talaria replicating the workload that Disrustor
/// performs in its benchmark.
///
/// Notably, one minor difference in workloads is that Talaria does not expose the "sequence" number
/// to the reservations. So, to still allow validating the assertions, Talaria will *also* increment
/// a local counter to maintain the "sequence" number on the receiver side. This should only
/// *negatively* impact Talaria's benchmark and seems to be a fair compromise.
pub fn run_disruptor_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let start = Instant::now();

            disrustor_channel(
                channel_size,
                black_box(iterations),
                chunk_size as u64,
                |d| SingleProducerSequencer::new(d.buffer_size(), SpinLoopWaitStrategy::new()),
            );

            start.elapsed()
        });
    });
}

pub fn run_exclusive_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    const MAIN_PARTITION: usize = 0;
    const THREAD_PARTITION: usize = 1;

    fn run_receiver(mut partition: Partition<Exclusive, i64>, chunk: usize, amount: usize) {
        let mut counter = 0;

        for _ in 0..amount {
            let mut reservation = partition.reserve(chunk).unwrap();

            for msg in reservation.iter() {
                assert_eq!(*msg, counter);
                counter += 1;
            }
        }
    }

    fn sender(mut partition: Partition<Exclusive, i64>, chunk: usize, amount: usize) {
        let mut counter = 0;

        for _ in 0..amount {
            let mut reservation = partition.reserve(chunk).unwrap();

            for msg in reservation.iter_mut() {
                *msg = counter;
                counter += 1;
            }
        }
    }

    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size,
                chunk_size,
            } = *args;

            let mut objects = vec![0i64; channel_size];
            objects
                .iter_mut()
                .enumerate()
                .for_each(|(idx, elem)| *elem = idx as i64);

            let channel = Channel::builder()
                .add_exclusive_partition()
                .add_exclusive_partition()
                .build(objects)
                .unwrap();

            let main_partition = channel.get_exclusive_partition(MAIN_PARTITION).unwrap();
            let thread_partition = channel.get_exclusive_partition(THREAD_PARTITION).unwrap();

            let receiver_thread = std::thread::spawn(move || {
                run_receiver(thread_partition, chunk_size, iterations as usize)
            });

            let start = Instant::now();
            sender(main_partition, chunk_size, iterations as usize);
            receiver_thread.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_disrustor_benchmark(c: &mut Criterion) {
    bench_scenarios(c, "Disrustor", run_disruptor_benchmark_with_args, vec![
        BenchArgs::new(65536, 1),
        BenchArgs::new(65536, 100),
    ]);
}

fn run_exclusive_benchmark(c: &mut Criterion) {
    bench_scenarios(
        c,
        "Exclusive Partition (Disrustor workload)",
        run_exclusive_benchmark_with_args,
        vec![BenchArgs::new(65536, 1), BenchArgs::new(65536, 100)],
    );
}

criterion_group! {
    name = disrustor_bench;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_disrustor_benchmark, run_exclusive_benchmark
}

criterion_main!(disrustor_bench);
