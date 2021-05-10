use mininet_runtime::*;
use std::{
    io::Read,
    net::{IpAddr, SocketAddr, TcpListener},
};

fn main() {
    let bob_addr = SocketAddr::new(IpAddr::V4(BOB_IPV4), PORT_NO);
    println!("Bob addr: {}", bob_addr);
    let bob_listener = TcpListener::bind(bob_addr).unwrap();
    println!("Bob successfully bound");
    let (mut bob_stream, _alice_ip) = bob_listener.accept().unwrap();
    println!("Bob accepted");

    let mut buf = [0 as u8; 1500];
    let mut total_received: usize = 0;

    while total_received < TEST_DATA_LEN {
        println!("Waiting for data at Bob");
        total_received += bob_stream.read(&mut buf).unwrap();
        println!("Read data at Bob!!!");
    }
}
