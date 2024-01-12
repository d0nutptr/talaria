

// criterion_main!(bench_suite);
//
// criterion_group! {
//     name = bench_suite;
//     config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
//     targets =
//         run_exclusive_suite,
//         run_concurrent_suite,
//         run_mpsc_control_suite
// }
//
//
//
// fn run_exclusive_suite(c: &mut Criterion) {
//     run_suite(c, "Exclusive Partitions", exclusive::run_benchmark_with_args);
// }
//
// fn run_concurrent_suite(c: &mut Criterion) {
//     run_suite(c, "Concurrent Partitions", concurrent::run_benchmark_with_args);
// }
