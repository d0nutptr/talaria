mod common;

use std::time::Instant;
use criterion::{BenchmarkId, black_box, Criterion, criterion_group, criterion_main};
use pprof::criterion::{Output, PProfProfiler};
use crate::common::{
    BenchArgs,
    bench_scenarios,
};

pub fn run_benchmark_with_args(
    c: &mut Criterion,
    id: BenchmarkId,
    args: BenchArgs
) {
    const OBJECT_SIZE: usize = 64;

    c.bench_with_input(
        id,
        &args,
        |b, args | {
            b.iter_custom(|iterations| {
                let BenchArgs { channel_size, .. } = *args;
                let (sender, receiver) = std::sync::mpsc::sync_channel(channel_size);

                let reader_thread = std::thread::spawn(move || {
                    while let Ok(msg) = receiver.recv() {
                        black_box(msg);
                    }
                });

                let start = Instant::now();

                for _ in 0 .. iterations {

                    sender.send([0u8; OBJECT_SIZE]).expect("send failed");
                }
                drop(sender);

                reader_thread.join().unwrap();
                start.elapsed()
            });
        }
    );
}

fn run_benchmark(c: &mut Criterion) {
    // since mpsc doesn't expose an api for fetching multiple elements at a time (and batching would require allocating a vec),
    // we'll just ignore chunking for now in fairness to std::sync::mpsc
    bench_scenarios(c, "std::sync::mpsc", run_benchmark_with_args, vec![
        BenchArgs::new(1024, 1),
        BenchArgs::new(4096, 1),
    ]);
}

criterion_group! {
    name = mpsc_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(mpsc_channel);