#[test]
fn exclusive_counter() {
    use talaria::channel::Channel;

    // id of the partition we'll access on the main thread
    const MAIN_PARTITION: usize = 0;
    // id of the partition we'll access on worker thread
    const THREAD_PARTITION: usize = 1;

    let channel = Channel::builder()
        .add_exclusive_partition()
        .add_exclusive_partition()
        .build(vec![0u64; 16])
        .unwrap();

    let mut main_partition = channel.get_exclusive_partition(MAIN_PARTITION).unwrap();
    let mut thread_partition = channel.get_exclusive_partition(THREAD_PARTITION).unwrap();

    // spin up a worker thread
    // it will do what the main thread is doing, but in reverse..
    let thread_handle = std::thread::spawn(move || {
        while let Ok(mut reservation) = thread_partition.reserve(1) {
            if reservation[0] == 10 {
                break;
            } else {
                reservation[0] += 1;
            }
        }
    });

    // reserve an item at a time
    while let Ok(mut reservation) = main_partition.reserve(1) {
        if reservation[0] == 10 {
            break;
        } else {
            reservation[0] += 1;
        }
    }

    thread_handle.join().unwrap();
}