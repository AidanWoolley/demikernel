use catnip::{
    protocols::{ 
        ip,
        ipv4,
    },
    runtime::Runtime,
};
use futures::task::noop_waker_ref;
use mininet_runtime::*;
use std::{
    convert::TryFrom,
    future::Future,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
    time::Instant,
};
fn main() {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut alice = new_mininet_engine("alice", Instant::now(), ALICE_MAC, ALICE_IPV4);
    println!("Created Alice");
    // Establish the connection between the two peers.
    let port = ip::Port::try_from(PORT_NO).unwrap();
    let bob_addr = ipv4::Endpoint::new(BOB_IPV4, port);

    let alice_fd = alice.tcp_socket();
    println!("Created Alice TCP Socket");
    let mut connect_future = alice.tcp_connect(alice_fd, bob_addr);
    println!("Attempting to connect to Bob");

    // Actually complete connection
    loop {
        alice.rt().poll_scheduler();
        if let Some(data) = alice.rt().receive() {
            alice.receive(data).unwrap();
        }
        match Future::poll(Pin::new(&mut connect_future), &mut ctx) {
            Poll::Ready(Ok(())) => break,
            Poll::Pending => continue,
            Poll::Ready(_) => panic!("Connection failed at Alice..."),
        }
    }

    println!("Connection Established at Alice!!")

    // loop {
    //     self.rt.scheduler().poll();
    //     while let Some(pkt) = self.rt.receive() {
    //         if let Err(e) = self.engine.receive(pkt) {
    //             warn!("Dropped packet: {:?}", e);
    //         }
    //     }
    // }
}