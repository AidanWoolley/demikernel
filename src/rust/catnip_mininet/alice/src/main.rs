use catnip::{
    protocols::{
        ip,
        ipv4,
        tcp::congestion_ctrl
    },
    runtime::Runtime,
    sync::BytesMut
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
    time::Instant
};

fn main() {
    // Create async context object
    let mut ctx = Context::from_waker(noop_waker_ref());
    // Create alice engine to attempt socket connect and send data
    let mut alice = new_mininet_engine("alice", Instant::now(), ALICE_MAC, ALICE_IPV4);
    // alice.rt().disable_congestion_control();
    println!("Created Alice");

    // Establish the connection between the two peers.
    let port = ip::Port::try_from(PORT_NO).unwrap();
    let bob_addr = ipv4::Endpoint::new(BOB_IPV4, port);

    let alice_fd = alice.tcp_socket();
    println!("Created Alice TCP Socket");
    let mut connect_future = alice.tcp_connect(alice_fd, bob_addr);
    println!("Attempting to connect to Bob");
    
    let alice_cb;
    // Actually complete connection
    loop {
        alice.rt().poll_scheduler();
        if let Some(data) = alice.rt().receive() {
            match alice.receive(data) {
                Ok(_) => (),
                Err(fail) => println!("{}", fail.to_string()),
            }
        }
        
        match Future::poll(Pin::new(&mut connect_future), &mut ctx) {
            Poll::Ready(Ok(cb_handle)) => { alice_cb = cb_handle; break; },
            Poll::Pending => continue,
            Poll::Ready(_) => panic!("Connection failed at Alice..."),
        }
    }
    println!("Connection Established at Alice!!");
    
    let mut send_future = alice.tcp_push(alice_fd, BytesMut::zeroed(TEST_DATA_LEN).freeze());
    match Future::poll(Pin::new(&mut send_future), &mut ctx) {
        Poll::Ready(Ok(())) => (),
        _ => panic!("Error sending data"),
    }
    alice.rt().poll_scheduler();
    println!("PushFuture resolved");

    while alice_cb.sender.unsent_queue.borrow().len() != 0 || alice_cb.sender.unacked_queue.borrow().len() != 0 {
        alice.rt().poll_scheduler();
        alice.rt().advance_clock(Instant::now());
        
        if let Some(data) = alice.rt().receive() {
            match alice.receive(data) {
                Ok(_) => (),
                Err(fail) => println!("{}", fail.to_string()),
            }
            // println!("RTT Estimate {:?}", alice_cb.sender.rto.borrow().estimate());
        }

    }

    alice.tcp_close(alice_fd).unwrap();
    alice.rt().poll_scheduler();

    // loop {
    //     self.rt.scheduler().poll();
    //     while let Some(pkt) = self.rt.receive() {
    //         if let Err(e) = self.engine.receive(pkt) {
    //             warn!("Dropped packet: {:?}", e);
    //         }
    //     }
    // }
}