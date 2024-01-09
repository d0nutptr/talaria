#![cfg(loom)]

use std::sync::atomic::Ordering;
use std::time::Duration;
use talaria::channel::Channel;

const MAX_TEST_TIME: Duration = Duration::from_secs(3 * 60); // 3 minutes
const MAX_BRANCHES: usize = 25_000_000;

fn get_loom_testing_model(preemption_bound: impl Into<Option<usize>>) -> loom::model::Builder {
    let mut model = loom::model::Builder::default();

    model.preemption_bound = preemption_bound.into();
    model.max_branches = MAX_BRANCHES;
    model.max_duration = Some(MAX_TEST_TIME);

    model
}


fn check_partition_states_valid<T>(channel: &Channel<T>) {
    loom::stop_exploring();

    channel
        .introspect_partition_states()
        .iter()
        .map(|partition_state| {
            let inner = partition_state.introspect_inner();
            let partition_id = *inner.partition_id;
            let committed_index = inner.committed_index.load(Ordering::SeqCst);
            let reservation_index = inner.reserved_index.load(Ordering::SeqCst);
            let boundary_index = partition_state.introspect_read_boundary_index();

            (partition_id, committed_index, reservation_index, boundary_index)
        })
        .for_each(|(partition_id, committed_index, reservation_index, boundary_index)| {
            // some of these conditions are only true _most_ of the time (e.g. before we've rolled over virtual indexes)
            // but, given the limited capabilities of our exhaustive testing, we'll never see that condition so these are basically always true
            assert!(
                committed_index <= reservation_index,
                "Partition {partition_id} has violated constraint that reservation index is always greater than or equal to committed index."
            );

            if partition_id != 0 {
                assert!(
                    reservation_index <= boundary_index,
r#"A partition has violated the constraint that reservation index is always less than or equal to the boundary index.
    Partition: {partition_id}
    Committed index: {committed_index}
    Reservation: {reservation_index}
    Boundary index: {boundary_index}"#
                );
            }
        });

    loom::explore();
}

#[test]
fn exclusive_partitions_never_overlap() {
    const CHANNEL_SIZE: usize = 4;

    fn run(channel: Channel<usize>, partition_id: usize, amount: usize, chunk: usize) {
        let mut partition = channel.get_exclusive_partition(partition_id).unwrap();

        for _ in 0 .. amount / chunk {
            let _ = partition.reserve(chunk).unwrap();
            check_partition_states_valid(&channel);
        }
    }

    get_loom_testing_model(None).check(|| {
        let ping_channel = Channel::builder()
            .add_exclusive_partition()
            .add_exclusive_partition()
            .build(vec![0; CHANNEL_SIZE])
            .unwrap();

        let pong_channel = ping_channel.clone();

        let ping_handle = loom::thread::spawn(move || run(ping_channel, 0, 5, 1));
        let pong_handle = loom::thread::spawn(move || run(pong_channel, 1, 5, 1));

        ping_handle.join().unwrap();
        pong_handle.join().unwrap();
    });
}