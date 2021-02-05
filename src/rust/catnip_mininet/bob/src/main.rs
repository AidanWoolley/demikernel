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
    let mut bob = new_mininet_engine("bob", Instant::now(), BOB_MAC, BOB_IPV4);
    println!("Created Bob");
    // Establish the connection between the two peers.
    let port = ip::Port::try_from(PORT_NO).unwrap();
    let listen_addr = ipv4::Endpoint::new(BOB_IPV4, port);

    let listen_fd = bob.tcp_socket();
    println!("Created Bob TCP Socket");
    bob.tcp_bind(listen_fd, listen_addr).unwrap();
    println!("Bound Bob TCP Socket");
    bob.tcp_listen(listen_fd, 1).unwrap();
    println!("Listening on Bob TCP Socket");
    let mut accept_future = bob.tcp_accept(listen_fd);

    // Actually complete connection
    let bob_fd: u32;
    loop {
        bob.rt().poll_scheduler();
        if let Some(data) = bob.rt().receive() {
            bob.receive(data).unwrap();
        }
        match Future::poll(Pin::new(&mut accept_future), &mut ctx) {
            Poll::Ready(Ok(fd)) => {bob_fd = fd; break},
            Poll::Pending => continue,
            Poll::Ready(_) => panic!("Connection failed at Bob..."),
        }
    }

    println!("Connection Established at Bob!! Bob_fd = {}", bob_fd);

}
