use talaria::channel::Channel;

#[test]
fn concurrent_blocking_ping_pong() {
    const CHANNEL_SIZE: usize = 4;
    const MESSAGE_COUNT: usize = 8;

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Message {
        Ping,
        Pong,
    }

    fn run(channel: Channel<Message>, partition_id: usize, amount: usize, expectation: Message) {
        let mut partition = channel.get_concurrent_partition(partition_id).unwrap();

        for _ in 0..amount {
            let mut reservation = partition.reserve(1).unwrap();

            assert_eq!(
                reservation[0], expectation,
                "Thread expected a {expectation:#?}"
            );
            reservation[0] = match expectation {
                Message::Ping => Message::Pong,
                Message::Pong => Message::Ping,
            };
        }
    }

    shuttle::check_random(
        || {
            let ping_one_channel = Channel::builder()
                .add_concurrent_partition()
                .add_concurrent_partition()
                .build(vec![Message::Pong; CHANNEL_SIZE])
                .unwrap();

            let ping_two_channel = ping_one_channel.clone();
            let pong_channel = ping_one_channel.clone();

            let ping_one_handle = shuttle::thread::spawn(move || {
                run(ping_one_channel, 0, MESSAGE_COUNT, Message::Pong)
            });
            let ping_two_handle = shuttle::thread::spawn(move || {
                run(ping_two_channel, 0, MESSAGE_COUNT, Message::Pong)
            });
            let pong_handle = shuttle::thread::spawn(move || {
                run(pong_channel, 1, MESSAGE_COUNT * 2, Message::Ping)
            });

            ping_one_handle.join().unwrap();
            ping_two_handle.join().unwrap();
            pong_handle.join().unwrap();
        },
        100_000,
    );
}
