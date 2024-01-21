use talaria::partition::PartitionT;

#[test]
fn exclusive_counter() {
    use talaria::channel::Channel;

    const WORK_FACTOR: usize = 128;
    // id of the partition we'll access on the main thread
    const MAIN_PARTITION: usize = 0;
    // id of the partition we'll access on worker thread
    const THREAD_PARTITION: usize = 1;

    let channel = Channel::builder()
        .add_exclusive_partition()
        .add_exclusive_partition()
        .build(vec![0usize; WORK_FACTOR])
        .unwrap();

    let mut main_partition = channel.get_exclusive_partition(MAIN_PARTITION).unwrap();
    let mut thread_partition = channel.get_exclusive_partition(THREAD_PARTITION).unwrap();

    // spin up a worker thread
    // it will do what the main thread is doing, but in reverse..
    let thread_handle = std::thread::spawn(move || {
        'outer: while let Ok(mut reservation) = thread_partition.reserve(100) {
            for elem in reservation.iter_mut() {
                if *elem == WORK_FACTOR {
                    break 'outer;
                }

                *elem += 1;
            }
        }
    });

    'outer: while let Ok(mut reservation) = main_partition.reserve(100) {
        for elem in reservation.iter_mut() {
            if *elem == WORK_FACTOR {
                break 'outer;
            }

            *elem += 1;
        }
    }

    thread_handle.join().unwrap();
}
