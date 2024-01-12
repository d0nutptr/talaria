use criterion::{BenchmarkId, Criterion};

#[derive(Debug)]
pub struct BenchArgs {
    pub channel_size: usize,
    pub chunk_size: usize,
}

impl BenchArgs {
    pub fn new(channel_size: usize, chunk_size: usize) -> Self {
        Self {
            channel_size,
            chunk_size,
        }
    }
}

pub fn bench<'a, F: FnOnce(&mut Criterion, BenchmarkId, BenchArgs)>(
    function_name: &str,
    criterion: &'a mut Criterion,
    args: BenchArgs,
) -> impl FnOnce(F) + 'a {
    let input_description = format!(
        "Channel Size: {} elements, Reservation Size: {} elements",
        args.channel_size,
        args.chunk_size,
    );

    let id = BenchmarkId::new(function_name, input_description);

    move |bench_fn| bench_fn(criterion, id, args)
}

pub fn bench_scenarios<F>(c: &mut Criterion, suite_name: &str, bench_fn: F, inputs: Vec<BenchArgs>)
    where
        F: Fn(&mut Criterion, BenchmarkId, BenchArgs)
{
    inputs
        .into_iter()
        .for_each(|args| {
            bench(suite_name, c, args)(|c, id, args| bench_fn(c, id, args));
        });
}