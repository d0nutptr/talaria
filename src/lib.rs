/*!
# Talaria

Talaria is a high performance, cyclic message passing library with bounded FIFO semantics.

Talaria is broken up into three main concepts:
* [Channels](channel::Channel)
* [Partitions](partition::Partition)
* [Reservations](partition::Reservation)

#### Channels
Talaria's channels are similar to bounded channels found in the standard library. A couple of differences exist, though.

For one, channels need to be provided a fixed collection of objects to manage ahead of time. Secondly, channels are constructed
with at least two partitions.

#### Partitions
Partitions can be thought of as "distinct states" for the data managed by the channel. Why not consider partitions
as "distinct owners" of data? Partitions can be configured to either be [Exclusive](partition::Exclusive) or [Concurrent](partition::Concurrent),
which describes whether or not multiple instances of the partition can be held simultaneously. So, when all partitions are
in "exclusive" mode, it might make sense to consider them as distinct "owners", but concurrent access to a partition changes
this somewhat.

Partitions are used to reserve one or more objects currently owned by the partition. Once the objects are no longer in use
(e.g. go out of scope), they are "transferred" to the next logical partition. So, objects go from partition `0` to partition `1`,
from partition `1` to partition `2`, and so on and so forth. Once an object in the final partition is unreserved, it is
returned to partition `0`.

It's helpful to think of partitions more as "states" in a cycle, rather than "sender" and "receiver" pairs. In fact,
this was the original motivation for the library - to use partitions to represent states of an object in its lifecycle,
allowing for simple state transitions, and access by multiple owners if desired. This, of course, lends itself to message
passing, assuming mutability of the data inside the partitions.

#### Reservations
Reservations represent exclusive ownership of some data. Reservations are created from a partition. Once available, a
[reservation](partition::Reservation) is created which holds exclusive ownership to the objects requested. This means that
once you hold a reservation, it is completely safe to read and write to these objects.

Once the reservation goes out of scope, or is explicitly dropped, the ownership of the objects are "transferred" to the next
logical partition. Notably, for concurrent partitions, multiple reservations can be requested at a time on one partition.
Holding a reservation still guarantees exclusive ownership of the objects managed by the reservation, but dropping the
reservation can temporarily block execution until the prior reservations have also been passed along. That is to say,
reservations respect FIFO ordering - a reservation created first, must be dropped first and later reservations will wait
until this is possible.

# How to use

There are three steps to using Talaria:

1. Create a channel
2. Get a partition
3. Acquire a reservation

Creating a channel can be done via Channel's [builder](channel::Channel::builder) method. We'll need to pass in a fixed
collection of objects for it to manage when we call [build](channel::Builder::build).

Afterwards, depending on how we configured our partitions, we'll either call [get_exclusive_partition](channel::Channel::get_exclusive_partition)
or [get_concurrent_partition](channel::Channel::get_concurrent_partition) to get a partition. This will return a [partition](partition::Partition)
which we can use to acquire reservations.

Finally, we can call [reserve](partition::Partition::reserve) on the partition to get a reservation. Once we hold a reservation,
we can either index directly into the reservation or call [iter](partition::Reservation::iter) (or [iter_mut](partition::Reservation::iter_mut))
to get an iterator over the objects managed by the reservation.

Once dropped, the items managed by the reservation will be transferred to the next logical partition.

```
# use talaria::channel::Channel;
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
            // otherwise we got an unexpected message!
            Message::Pong => panic!("unexpected message!")
        };
    }

    thread_handle.join().unwrap();
}
```
 */

#![feature(non_null_convenience)]

pub mod channel;
pub mod error;
pub mod partition;
mod sync_types;