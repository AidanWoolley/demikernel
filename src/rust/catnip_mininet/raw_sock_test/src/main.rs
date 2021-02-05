use libc;
use socket2::*;
use std::{
    convert::TryInto,
    fs,
    mem,
    time::Duration,
    thread::sleep,
};
fn main() {
    const ETH_P_ALL: u16 = (libc::ETH_P_ALL as u16).to_be();
    let recv_socket = Socket::new(Domain::packet(), Type::raw(), Some((ETH_P_ALL as libc::c_int).into())).unwrap();
    let dest_addr_ll = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET.try_into().unwrap(),
        sll_protocol: ETH_P_ALL,
        sll_ifindex: fs::read_to_string("/sys/class/net/bob-eth0/ifindex").expect("Could not read ifindex").trim().parse().unwrap(),
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 0,
        sll_addr: [0; 8],

    };
    let dest_addr_ll_ptr: *const libc::sockaddr_ll = &dest_addr_ll;
    let dest_addr;
    unsafe {
        let dest_addr_ptr = mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr>(dest_addr_ll_ptr);
        dest_addr = SockAddr::from_raw_parts(dest_addr_ptr, mem::size_of::<libc::sockaddr_ll>().try_into().unwrap());
    }
    recv_socket.bind(&dest_addr).unwrap();

    loop {
        let mut buf: [u8; 10240] = [0; 10240];
        let read_result = recv_socket.recv_from(&mut buf[..]);
        match read_result {
            Ok((received_size, _origin)) => println!("Received some bytes: {:?}", buf[..received_size].to_vec()),
            Err(_) => (),
        }
        sleep(Duration::new(1, 0));
    }
}