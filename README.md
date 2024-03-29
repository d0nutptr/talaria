# talaria

`talaria` is a high performance, cyclic message passing library with bounded FIFO semantics.

> [!CAUTION]
> While `talaria` has been validated with *some* correctness tests, it should still be considered unstable and unproven at this time.

`talaria` is broken up into three main concepts:
* Channels
* Partitions
* Reservations

# Channels
`talaria`'s channels are similar to bounded channels found in the standard library. A couple of
differences exist, though.

For one, channels need to be provided a fixed collection of objects to manage ahead of time.
Secondly, channels are constructed with at least two partitions.

# Partitions
Partitions can be thought of as "states" for the data managed by the channel. 
Why not consider partitions as "owners" of data? Well, partitions can be configured to either be
`exclusive` or `concurrent`, which describes whether multiple instances of the partition can be held simultaneously.
So, when all partitions are in "exclusive" mode, it might make sense to consider them as distinct "owners",
but concurrent access to a partition changes this somewhat. The data is managed by the partition, but holding
a reference to a partition (_unless it's an exclusive partition_) does not guarantee that the data is exclusively
owned by this context.

Partitions are used to reserve one or more objects currently owned by the partition. Once the
objects are no longer in use (e.g. go out of scope), they are "transferred" to the next logical
partition. So, objects go from partition `0` to partition `1`, from partition `1` to partition
`2`, and so on and so forth. Once an object in the final partition is unreserved, it is returned
to partition `0`.

It's helpful to think of partitions more as "states" in a cycle, rather than "sender" and
"receiver" pairs. In fact, this was the original motivation for the library - to use partitions
to represent states of an object in its lifecycle, allowing for simple state transitions, and
access by multiple owners if desired. This, of course, lends itself to message passing, assuming
mutability of the data inside the partitions.

# Reservations
Reservations represent exclusive ownership of some data. Reservations are created from a
partition. Once available, a [reservation](partition::Reservation) is created which holds
exclusive ownership to the objects requested. This means that once you hold a reservation, it is
completely safe to read and write to these objects.

Once the reservation goes out of scope, or is explicitly dropped, the ownership of the objects
are "transferred" to the next logical partition. Notably, for concurrent partitions, multiple
reservations can be requested at a time on one partition. Holding a reservation still guarantees
exclusive ownership of the objects managed by the reservation, but dropping the reservation can
temporarily block execution until the prior reservations have also been passed along. That is to
say, reservations respect FIFO ordering - a reservation created first, must be dropped first and
later reservations will wait until this is possible.

# How to use

There are three steps to using `talaria`:

1. Create a channel
2. Get a partition
3. Acquire a reservation

Here's an example of using `talaria` to pass "Ping/Pong" messages between two threads, indefinitely.

```rust
use talaria::channel::Channel;

#[derive(Clone)]
enum Message {
    Ping, 
    Pong,
}

fn main() {
    // id of the partition we'll access on the main thread
    const MAIN_PARTITION: usize = 0;
    // id of the partition we'll access on worker thread
    const THREAD_PARTITION: usize = 1;
    
    let channel = Channel::builder()
        .add_exclusive_partition()
        .add_exclusive_partition()
        .build(vec![Message::Ping; 16])
        .unwrap();

    let channel_clone = channel.clone();
    
    // spin up a worker thread
    // it will do what the main thread is doing, but in reverse..
    let thread_handle = std::thread::spawn(move || {
        let mut partition = channel_clone
            .get_exclusive_partition(THREAD_PARTITION)
            .unwrap();
        
        while let Ok(mut reservation) = partition.reserve(1) {
            reservation[0] = match &reservation[0] {
                Message::Pong => Message::Ping,
                Message::Ping => panic!("unexpected message!")
            };
        }
    });

    let mut partition = channel
        .get_exclusive_partition(MAIN_PARTITION)
        .unwrap();

    // reserve an item at a time
    while let Ok(mut reservation) = partition.reserve(1) {
        reservation[0] = match &reservation[0] {
            // if the first (and only) element is a "ping" message,
            // set it to "pong" and forward it
            Message::Ping => Message::Pong,
            //  otherwise we got an unexpected message!
            Message::Pong => panic!("unexpected message!")
        };
    }
    
    thread_handle.join().unwrap();
}
```

# Benchmarking

`talaria` uses Criterion for benching and only requires you run `cargo bench` to begin benchmarking.

Benchmarks are written for both exclusive and concurrent two-thread partition scenarios, as well as
the equivalent tests with `std::sync::mpsc` and `crossbeam`'s bounded channels.

The following is a sample of benchmarking on my machine (i9-9900k, 64Gb 3200mhz RAM):

## Exclusive
![Exclusive Partition Benchmark](imgs/exclusive.svg)

## Concurrent
![Concurrent Partition Benchmark](imgs/concurrent.svg)

## std::sync::mpsc
![std::sync::mpsc Benchmark](imgs/mpsc.svg)

## crossbeam
![crossbeam Benchmark](imgs/crossbeam.svg)

# Correctness Tests
`talaria` comes with a (relatively incomplete) suite of correctness tests using both [`loom`](https://github.com/tokio-rs/loom)
and [`shuttle`](https://github.com/awslabs/shuttle). To run them, do the following:

## Loom
`RUSTFLAGS="--cfg loom" cargo test`

## Shuttle
`RUSTFLAGS="--cfg shuttle" cargo test`

Some tests are timebound to prevent them from running either indefinitely or an excessively long while. Expect tests to take a few minutes, though.
