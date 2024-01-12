#![cfg(loom)]

use loom::model::Builder;
use std::time::Duration;
use talaria::channel::Channel;

const MAX_TEST_TIME: Duration = Duration::from_secs(3 * 60); // 3 minutes
const MAX_BRANCHES: usize = 25_000_000;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Message {
    Ping,
    Pong,
}

fn get_loom_testing_model(preemption_bound: impl Into<Option<usize>>) -> Builder {
    let mut model = loom::model::Builder::default();

    model.preemption_bound = preemption_bound.into();
    model.max_branches = MAX_BRANCHES;
    model.max_duration = Some(MAX_TEST_TIME);

    model
}

#[test]
fn exclusive_blocking_ping_pong() {
    const CHANNEL_SIZE: usize = 2;
    const MESSAGE_COUNT: usize = 4;

    fn run(channel: Channel<Message>, partition_id: usize, amount: usize, expectation: Message) {
        let mut partition = channel.get_exclusive_partition(partition_id).unwrap();

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

    get_loom_testing_model(None).check(|| {
        let ping_channel = Channel::builder()
            .add_exclusive_partition()
            .add_exclusive_partition()
            .build(vec![Message::Pong; CHANNEL_SIZE])
            .unwrap();

        let pong_channel = ping_channel.clone();

        let ping_handle =
            loom::thread::spawn(move || run(ping_channel, 0, MESSAGE_COUNT, Message::Pong));
        let pong_handle =
            loom::thread::spawn(move || run(pong_channel, 1, MESSAGE_COUNT, Message::Ping));

        ping_handle.join().unwrap();
        pong_handle.join().unwrap();
    });
}

#[test]
fn exclusive_nonblocking_ping_pong() {
    const CHANNEL_SIZE: usize = 2;
    const MESSAGE_COUNT: usize = 2;

    fn run(channel: Channel<Message>, partition_id: usize, amount: usize, expectation: Message) {
        let mut partition = channel.get_exclusive_partition(partition_id).unwrap();

        for _ in 0..amount {
            let mut did_retry = false;
            let mut reservation = loop {
                match partition.try_reserve(1) {
                    Ok(reservation) => break reservation,
                    // skip this branch so our test completes relatively quickly
                    _ if did_retry => loom::skip_branch(),
                    _ => {
                        did_retry = true;
                        loom::hint::spin_loop();
                        loom::thread::yield_now();
                    }
                }
            };

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

    get_loom_testing_model(None).check(|| {
        let ping_channel = Channel::builder()
            .add_exclusive_partition()
            .add_exclusive_partition()
            .build(vec![Message::Pong; CHANNEL_SIZE])
            .unwrap();

        let pong_channel = ping_channel.clone();

        let ping_handle =
            loom::thread::spawn(move || run(ping_channel, 0, MESSAGE_COUNT, Message::Pong));
        let pong_handle =
            loom::thread::spawn(move || run(pong_channel, 1, MESSAGE_COUNT, Message::Ping));

        ping_handle.join().unwrap();
        pong_handle.join().unwrap();
    });
}

/*
   Note! The concurrent tests use more than 2 threads in their tests to properly exercise the concurrent partitions.
   We want to test scenarios where at least 2 threads are fighting over a partition, and need a third thread to ensure
   the first partition is properly fed. It also lets us test the boundary conditions around internal ring index rollover.

   Unfortunately, loom seems to struggle with more than 2 threads at a time (see: https://github.com/tokio-rs/loom/issues/53)

   This is especially true, because, strictly speaking, talaria is not guaranteed to make progress always:
       e.g. channel of size 8 split evenly between two partitions, while they block asking for 5 elements

   Therefore it's possible to encounter scenarios, based on thread scheduling, where a worst-case scenario occurs.

   Anyway, the tl;dr is that these tests limit the number of messages they send and enforce a timeout (see `MAX_TEST_TIME`)
   but this *should* be sufficient to catch most bugs.
*/

#[test]
fn concurrent_blocking_ping_pong() {
    const CHANNEL_SIZE: usize = 2;
    const MESSAGE_COUNT: usize = 4;

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

    get_loom_testing_model(None).check(|| {
        let ping_one_channel = Channel::builder()
            .add_concurrent_partition()
            .add_concurrent_partition()
            .build(vec![Message::Pong; CHANNEL_SIZE])
            .unwrap();

        let ping_two_channel = ping_one_channel.clone();
        let pong_channel = ping_one_channel.clone();

        let ping_one_handle =
            loom::thread::spawn(move || run(ping_one_channel, 0, MESSAGE_COUNT, Message::Pong));
        let ping_two_handle =
            loom::thread::spawn(move || run(ping_two_channel, 0, MESSAGE_COUNT, Message::Pong));
        let pong_handle =
            loom::thread::spawn(move || run(pong_channel, 1, MESSAGE_COUNT * 2, Message::Ping));

        ping_one_handle.join().unwrap();
        ping_two_handle.join().unwrap();
        pong_handle.join().unwrap();
    });
}

#[test]
fn concurrent_nonblocking_ping_pong() {
    const CHANNEL_SIZE: usize = 2;
    const MESSAGE_COUNT: usize = 1;

    #[derive(Copy, Clone, Debug, PartialEq)]
    enum Message {
        Ping,
        Pong,
    }

    fn run(channel: Channel<Message>, partition_id: usize, amount: usize, expectation: Message) {
        let mut partition = channel.get_concurrent_partition(partition_id).unwrap();

        for _ in 0..amount {
            let mut did_retry = false;
            let mut reservation = loop {
                match partition.try_reserve(1) {
                    Ok(reservation) => break reservation,
                    // skip this branch so our test completes relatively quickly
                    _ if did_retry => loom::skip_branch(),
                    _ => {
                        did_retry = true;
                        loom::hint::spin_loop();
                        loom::thread::yield_now();
                    }
                }
            };

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

    get_loom_testing_model(1).check(|| {
        let ping_one_channel = Channel::builder()
            .add_concurrent_partition()
            .add_concurrent_partition()
            .build(vec![Message::Pong; CHANNEL_SIZE])
            .unwrap();

        let ping_two_channel = ping_one_channel.clone();
        let pong_channel = ping_one_channel.clone();

        let ping_one_handle =
            loom::thread::spawn(move || run(ping_one_channel, 0, MESSAGE_COUNT, Message::Pong));
        let ping_two_handle =
            loom::thread::spawn(move || run(ping_two_channel, 0, MESSAGE_COUNT, Message::Pong));
        let pong_handle =
            loom::thread::spawn(move || run(pong_channel, 1, MESSAGE_COUNT, Message::Ping));

        ping_one_handle.join().unwrap();
        ping_two_handle.join().unwrap();
        pong_handle.join().unwrap();
    });
}
