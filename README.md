# Crossfire

[![Build Status](https://github.com/frostyplanet/crossfire-rs/workflows/Rust/badge.svg)](
https://github.com/frostyplanet/crossfire-rs/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](
https://github.com/qignstor/crossfire-rs#license)
[![Cargo](https://img.shields.io/crates/v/crossfire.svg)](
https://crates.io/crates/crossfire)
[![Documentation](https://docs.rs/crossfire/badge.svg)](
https://docs.rs/crossfire)
[![Rust 1.36+](https://img.shields.io/badge/rust-1.36+-lightgray.svg)](
https://www.rust-lang.org)

High-performance spsc/mpsc/mpmc channels.

It supports async context, or communicates between async-blocking context.

Implemented with lockless in mind, low level is based on crossbeam-channel.
For the concept, please refer to [wiki](https://github.com/frostyplanet/crossfire-rs/wiki).


## Stability and versions

Crossfire v1.0 has been released and used in production since 2022.12. Heavily tested on X86_64 and ARM.

V2.0 has refactored the codebase and API at 2025.6. By removing generic types of ChannelShared object in sender and receiver,
it's easier to remember and code.

V2.0.x branch will remain in maintenance mode. Further optimization might be in v2.x_beta
version until long run tests prove to be stable.

## Performance

We focus on optimization of async logic, outperforming other async capability channels in most cases.

Due to context switching between sleep and wake, there is a certain
overhead on async context over crossbeam-channel which in blocking context.

Benchmark is written in criterion framework. You can run benchmark by:

```
cargo bench --bench crossfire
```

More benchmark data is on [wiki](https://github.com/frostyplanet/crossfire-rs/wiki/benchmark-v2.0.14-2025%E2%80%9008%E2%80%9003). Here are some of the results:

<img src="https://github.com/frostyplanet/crossfire-rs/wiki/images/benchmark-2.0.14-2025-08-03/mpsc_size_100_async.png" alt="mpsc bounded size 100 async context">

<img src="https://github.com/frostyplanet/crossfire-rs/wiki/images/benchmark-2.0.14-2025-08-03/mpmc_size_100_async.png" alt="mpmc bounded size 100 async context">

<img src="https://github.com/frostyplanet/crossfire-rs/wiki/images/benchmark-2.0.14-2025-08-03/mpmc_unbounded_async.png" alt="mpmc unbounded async context">


## APIs

### modules and functions

There are 3 modules: [spsc](https://docs.rs/crossfire/latest/crossfire/spsc/index.html), [mpsc](https://docs.rs/crossfire/latest/crossfire/mpsc/index.html), [mpmc](https://docs.rs/crossfire/latest/crossfire/mpmc/index.html), providing functions to allocate different types of channels.

The SP or SC interface, only for non-concurrent operation, it's more memory efficient than MP or MC implementations, and sometimes slightly faster.

The return types in these 3 modules are different:

* mpmc::bounded_blocking() : (tx blocking, rx blocking)

* mpmc::bounded_async() :  (tx async, rx async)

* mpmc::bounded_tx_async_rx_blocking() : (tx async, rx blocking)

* mpmc::bounded_tx_blocking_rx_async() : (tx blocking, rx async)

* mpmc::unbounded_blocking() : (tx non-blocking, rx blocking)

* mpmc::unbounded_async() : (tx non-blocking, rx async)


> **NOTE** :  For bounded channel, 0 size case is not supported yet. (Temporary rewrite as 1 size).

### Types

<table align="center" cellpadding="30">
<tr> <th rowspan="2"> Context </th><th colspan="2" align="center"> Sender (Producer) </th> <th colspan="2" align="center"> Receiver (Consumer) </th> </tr>
<tr> <td> Single </td> <td> Multiple </td><td> Single </td><td> Multiple </td></tr>
<tr><td rowspan="2"> <b>Blocking</b> </td>
<td colspan="2" align="center"> BlockingTxTrait </td>
<td colspan="2" align="center"> BlockingRxTrait </td></tr>
<tr>
<td align="center">Tx </td>
<td align="center">MTx</td>
<td align="center">Rx</td>
<td align="center">MRx</td> </tr>

<tr><td rowspan="2"><b>Async</b></td>
<td colspan="2" align="center">AsyncTxTrait</td>
<td colspan="2" align="center">AsyncRxTrait</td></tr>
<tr>
<td>AsyncTx</td>
<td>MAsyncTx</td>
<td>AsyncRx</td>
<td>MAsyncRx</td></tr>

</table>

> **NOTE**: For SP / SC version [AsyncTx], [AsyncRx], [Tx], [Rx], is not `Clone`, and without `Sync`,
Although can be moved to other thread, but not allowed to use send/recv while in Arc. (Refer to the compile_fail
examples in type document).

### Error types

Error types are re-exported from crossbeam-channel:  `TrySendError`, `SendError`, `TryRecvError`, `RecvError`

### Feature flags

- `tokio`: Enable send_timeout, recv_timeout API for async context, based on `tokio`.

- `async_std`: Enable send_timeout, recv_timeout API for async context, base on `async-std`.

### Async compatibility

Tested on tokio-1.x and async-std-1.x, by default we do not depend on any async runtime.

In async context, tokio-select! or future-select! can be used.  Cancelling is supported. You can combine
recv() future with tokio::time::timeout.

When feature "tokio" or "async_std" enable, we also provide two additional functions:

[send_timeout](https://docs.rs/crossfire/latest/crossfire/struct.AsyncTx.html#method.send_timeout) which will return the message failed to sent in [SendTimeoutError](https://docs.rs/crossfire/latest/crossfire/enum.SendTimeoutError.html).

[recv_timeout](https://docs.rs/crossfire/latest/crossfire/struct.AsyncRx.html#method.recv_timeout)

While using MAsyncTx or MAsyncRx, there's memory overhead to pass along small size wakers
for pending async producer or consumer. Because we aim to be lockless,
when the sending/receiving futures are cancelled (like tokio::time::timeout()),
might trigger immediate cleanup if non-conflict conditions are met.
Otherwise will rely on lazy cleanup. (waker will be consumed by actual message send and recv).

Never the less, for close notification without sending anything,
I suggest that use `tokio::sync::oneshot` instead.

## Usage

Cargo.toml:
```toml
[dependencies]
crossfire = "2.0"
```

# Example with tokio::select!

```rust
extern crate crossfire;
use crossfire::*;
#[macro_use]
extern crate tokio;
use tokio::time::{sleep, interval, Duration};

#[tokio::main]
async fn main() {
    let (tx, rx) = mpsc::bounded_async::<i32>(100);
    for _ in 0..10 {
        let _tx = tx.clone();
        tokio::spawn(async move {
            for i in 0i32..10 {
                let _ = _tx.send(i).await;
                sleep(Duration::from_millis(100)).await;
                println!("sent {}", i);
            }
        });
    }
    drop(tx);
    let mut inv = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = inv.tick() =>{
                println!("tick");
            }
            r = rx.recv() => {
                if let Ok(_i) = r {
                    println!("recv {}", _i);
                } else {
                    println!("rx closed");
                    break;
                }
            }
        }
    }
}
```
