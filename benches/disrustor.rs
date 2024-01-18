mod common;

use std::sync::Arc;
use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use disrustor::internal::{BatchEventProcessor, RingBuffer, SingleProducerSequencer, SpinLoopWaitStrategy, ThreadedExecutor};
use disrustor::{DataProvider, EventHandler, EventProcessorExecutor, ExecutorHandle, Sequence, Sequencer, WaitStrategy};
use disrustor::EventProcessorMut;
use pprof::criterion::{Output, PProfProfiler};

use crate::common::{bench_scenarios, BenchArgs, DEFAULT_OBJECT_SIZE};

struct Checker;

impl EventHandler<i64> for Checker {
    fn handle_event(&mut self, event: &i64, seq: Sequence, _: bool) {
        assert_eq!(*event, seq);
    }
}

// copied, mostly, from: https://github.com/sklose/disrustor/blob/e6a3e9226aa262fd916bb476b1c9c1d33b36d141/benches/u64_channels.rs#L34
// instead of 2^16 capacity, using passed in capacity
fn disrustor_channel<S: Sequencer, F: FnOnce(&RingBuffer<i64>) -> S>(capacity: usize, iterations: u64, chunk_size: u64, f: F) {
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
}


pub fn run_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size, chunk_size,
            } = *args;

            let start = Instant::now();
            disrustor_channel(channel_size, black_box(iterations), chunk_size as u64, |d| {
                SingleProducerSequencer::new(d.buffer_size(), SpinLoopWaitStrategy::new())
            });
            start.elapsed()
        });
    });
}

fn run_benchmark(c: &mut Criterion) {
    // since mpsc doesn't expose an api for fetching multiple elements at a time,
    // we'll just ignore chunking for now in fairness to std::sync::mpsc
    bench_scenarios(c, "disrustor", run_benchmark_with_args, vec![
        BenchArgs::new(65536, 1),
        BenchArgs::new(65536, 100),
    ]);
}

criterion_group! {
    name = disrustor_bench;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(disrustor_bench);
