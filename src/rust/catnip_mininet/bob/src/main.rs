use catnip::{
    protocols::{ 
        ip,
        ipv4
    },
    runtime::Runtime,
};
use futures::task::noop_waker_ref;
use mininet_runtime::*;
use std::{
    convert::TryFrom,
    future::Future,
    pin::Pin,
    num::Wrapping,
    task::{
        Context,
        Poll,
    },
    time::{Duration, Instant},
    thread::sleep,
};

fn main() {
    // Create async context object
    let mut ctx = Context::from_waker(noop_waker_ref());

    // Create the Bob engine to listen for connection and receive data
    let mut bob = new_mininet_engine("bob", Instant::now(), BOB_MAC, BOB_IPV4);

    println!("Created Bob");
    
    // Create listening socket.
    let port = ip::Port::try_from(PORT_NO).unwrap();
    let listen_addr = ipv4::Endpoint::new(BOB_IPV4, port);
    let bob_fd = bob.tcp_socket();
    println!("Created Bob TCP Socket");
    bob.tcp_bind(bob_fd, listen_addr).unwrap();
    println!("Bound Bob TCP Socket");
    bob.tcp_listen(bob_fd, 1).unwrap();
    println!("Listening on Bob TCP Socket");
    // Future will complete once a connection has been made
    let mut accept_future = bob.tcp_accept(bob_fd);

    // Actually complete connection
    let bob_fd: u32;
    let bob_cb;
    loop {
        // Each loop have to poll the scheduler to run background tasks (which will actually make the connection)
        // and update the Runtime object clock so that ACKs get sent at the appropriate time, etc.
        bob.rt().poll_scheduler();
        bob.rt().advance_clock(Instant::now());
        // Have to receive the bytes from the Socket in the runtime and manually pass them to the network stack for processing
        if let Some(data) = bob.rt().receive() {
            match bob.receive(data) {
                Ok(_) => (),
                Err(fail) => println!("{}", fail.to_string()),
            }
        }
        // Poll future every loop to see if a connection has been made
        match Future::poll(Pin::new(&mut accept_future), &mut ctx) {
            Poll::Ready(Ok((fd, cb_handle))) => {bob_fd = fd; bob_cb = cb_handle; break},
            Poll::Pending => continue,
            Poll::Ready(_) => panic!("Connection failed at Bob..."),
        }
    }

    println!("Connection Established at Bob!! Bob_fd = {}", bob_fd);
    
    let mut total_received: usize = 0;
    while total_received < TEST_DATA_LEN {
        bob.rt().poll_scheduler();
        bob.rt().advance_clock(Instant::now());
        if let Some(data) = bob.rt().receive() {
            match bob.receive(data) {
                Ok(_) => (),
                Err(fail) => println!("{}", fail.to_string()),
            }
        }
        // if bob_cb.receiver.recv_seq_no.get() - bob_cb.receiver.base_seq_no.get() > Wrapping(60000) {
            let mut pop_future = bob.tcp_pop(bob_fd);
            match Future::poll(Pin::new(&mut pop_future), &mut ctx) {
                Poll::Ready(Ok(bytes)) => {
                    total_received += bytes.len();
                    // println!("Received {} bytes. Total Received: {} bytes", bytes.len(), total_received);
                },
                Poll::Pending => (),
                _ => panic!("Error receiving bytes")
            };
        // }
    }

    // Wait until the last ACK is sent to close the connection 
    while bob_cb.receiver.ack_deadline.get() != None {
        bob.rt().advance_clock(Instant::now());
        bob.rt().poll_scheduler();
        sleep(Duration::from_millis(1));
    }
}
