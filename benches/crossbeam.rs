mod common;

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pprof::criterion::{Output, PProfProfiler};

use crate::common::bench_util::{bench_scenarios, BenchArgs, BenchmarkMessageType};

pub fn run_benchmark_with_args(c: &mut Criterion, id: BenchmarkId, args: BenchArgs) {
    c.bench_with_input(id, &args, |b, args| {
        b.iter_custom(|iterations| {
            let BenchArgs {
                channel_size, ..
            } = *args;
            let (sender, receiver) = crossbeam_channel::bounded(channel_size);

            let reader_thread = std::thread::spawn(move || {
                while let Ok(msg) = receiver.recv() {
                    black_box(msg);
                }
            });

            let start = Instant::now();

            for _ in 0..iterations {
                sender
                    .send(black_box(BenchmarkMessageType::default()))
                    .expect("send failed");
            }

            drop(sender);

            reader_thread.join().unwrap();
            start.elapsed()
        });
    });
}

fn run_benchmark(c: &mut Criterion) {
    // since mpsc doesn't expose an api for fetching multiple elements at a time,
    // we'll just ignore chunking for now in fairness to std::sync::mpsc
    bench_scenarios(c, "crossbeam-channel", run_benchmark_with_args, vec![
        BenchArgs::new(1, 1),
        BenchArgs::new(1024, 1),
        BenchArgs::new(4096, 1),
    ]);
}

criterion_group! {
    name = crossbeam_channel;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = run_benchmark
}

criterion_main!(crossbeam_channel);
