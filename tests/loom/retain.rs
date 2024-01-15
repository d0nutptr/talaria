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

#[test]
fn retained_partition_reduces_committed_index_march() {
    const CHANNEL_SIZE: usize = 4;

    get_loom_testing_model(None).check(|| {
        let sender_channel = Channel::builder()
            .add_exclusive_partition()
            .add_exclusive_partition()
            .build(vec![0u64; CHANNEL_SIZE])
            .unwrap();

        let receiver_channel = sender_channel.clone();

        let sender_handle = loom::thread::spawn(move || {
            let mut partition = sender_channel.get_exclusive_partition(0).unwrap();

            // reserve two elements.
            let mut reservation = partition.reserve(2).unwrap();

            // validate only 2 elements are present in the reservation
            assert_eq!(reservation.len(), 2, "reservation is not the proper size");

            // retain only 1 element
            reservation.retain(1).unwrap();

            // check that the reservation is smaller now
            assert_eq!(reservation.len(), 1, "reservation did not retain properly");

            // update all "reserved" elements to the value `1`
            reservation.iter_mut().for_each(|elem| *elem = 1);

            // drop the reservation, which should only increment the committed index by one
        });

        let receiver_handle = loom::thread::spawn(move || {
            let mut partition = receiver_channel.get_exclusive_partition(1).unwrap();

            let reservation = partition.reserve(1).unwrap();

            // enforce that the reservation only has 1 element
            assert_eq!(
                reservation.len(),
                1,
                "reservation is not the proper size on the second thread"
            );

            // enforce the reservation value is accurate
            assert_eq!(
                reservation[0], 1,
                "value expected to be `1` as assigned by the first thread"
            );

            // drop the reservation
            drop(reservation);

            // we should not be able to _attempt_ to reserve another element.
            // this will succeed, if retain failed to work properly
            // this will "fail", if retain only moved the pointer forward once
            assert!(
                partition.try_reserve(1).is_err(),
                "reservation should have failed"
            );
        });

        sender_handle.join().unwrap();
        receiver_handle.join().unwrap();
    });
}
