// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use catnip::{  
    engine::Engine,
    protocols::{
        arp,
        ethernet2::{frame::Ethernet2Header, MacAddress},
        tcp,
    },
    runtime::{
        PacketBuf,
        Runtime,
    },
    scheduler::{
        Operation,
        Scheduler,
        SchedulerHandle,
    },
    sync::{
        Bytes,
        BytesMut,
    },
    timer::{
        Timer,
        TimerRc,
        WaitFuture,
    },
};
use futures::{
    FutureExt,
};
use libc;
use rand::{
    distributions::{
        Distribution,
        Standard,
    },
    rngs::SmallRng,
    Rng,
    SeedableRng,
};
use socket2::*; 
use std::{
    cell::RefCell,
    convert::TryInto,
    fs,
    future::Future,
    mem,
    net::Ipv4Addr,
    rc::Rc,
    time::{
        Duration,
        Instant,
    },
};

pub type MininetEngine = Engine<MininetRuntime>;

pub const ALICE_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xa1]);
pub const ALICE_IPV4: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
pub const BOB_MAC: MacAddress = MacAddress::new([0x12, 0x23, 0x45, 0x67, 0x89, 0xb0]);
pub const BOB_IPV4: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 2);
pub const PORT_NO: u16 = 8000;
pub const ETH_P_ALL: u16 = (libc::ETH_P_ALL as u16).to_be();

#[derive(Clone)]
pub struct MininetRuntime {
    inner: Rc<RefCell<Inner>>,
    scheduler: Scheduler<Operation<MininetRuntime>>,
}

impl MininetRuntime {
    pub fn new(
        name: &str,
        now: Instant,
        link_addr: MacAddress,
        ipv4_addr: Ipv4Addr,
    ) -> Self {
        let mut arp_options = arp::Options::default();
        arp_options.retry_count = 2;
        arp_options.cache_ttl = Duration::from_secs(600);
        arp_options.request_timeout = Duration::from_secs(1);
        arp_options.initial_values.insert(ALICE_MAC, ALICE_IPV4);
        arp_options.initial_values.insert(BOB_MAC, BOB_IPV4);

        let socket = Socket::new(Domain::packet(), Type::raw(), Some((ETH_P_ALL as libc::c_int).into())).unwrap();
        socket.set_read_timeout(Some(Duration::new(0, 1000000))).unwrap();
        let ifindex: i32 = fs::read_to_string(format!("/sys/class/net/{}-eth0/ifindex", name)).expect("Could not read ifindex").trim().parse().unwrap();

        let bind_sockaddr_ll = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET.try_into().unwrap(),
            sll_protocol: ETH_P_ALL,
            sll_ifindex: ifindex,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
    
        };
        let bind_sockaddr_ll_ptr: *const libc::sockaddr_ll = &bind_sockaddr_ll;
        let bind_sockaddr;
        unsafe {
            let bind_sockaddr_ptr = mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr>(bind_sockaddr_ll_ptr);
            bind_sockaddr = SockAddr::from_raw_parts(bind_sockaddr_ptr, mem::size_of::<libc::sockaddr_ll>().try_into().unwrap());
        }
        socket.bind(&bind_sockaddr).unwrap();


        let inner = Inner {
            timer: TimerRc(Rc::new(Timer::new(now))),
            rng: SmallRng::from_seed([0; 16]),
            socket,
            link_addr,
            ipv4_addr,
            ifindex,
            tcp_options: tcp::Options::default(),
            arp_options,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
            scheduler: Scheduler::new(),
        }
    }

    pub fn poll_scheduler(&self) {
        // let mut ctx = Context::from_waker(noop_waker_ref());
        self.scheduler.poll();
    }
}

struct Inner {
    timer: TimerRc,
    rng: SmallRng,
    socket: Socket,
    link_addr: MacAddress,
    ipv4_addr: Ipv4Addr,
    ifindex: i32,
    tcp_options: tcp::Options,
    arp_options: arp::Options,
}

impl Runtime for MininetRuntime {
    type WaitFuture = WaitFuture<TimerRc>;

    #[allow(unused)]
    fn transmit(&self, pkt: impl PacketBuf) {
        let size = pkt.compute_size();
        let mut buf = BytesMut::zeroed(size);
        pkt.serialize(&mut buf[..]);
        let buf = buf.freeze();
        let (header, _) = Ethernet2Header::parse(buf.clone()).unwrap();
        let dest_addr_arr = header.dst_addr.to_array();
        let dest_addr_ll = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET.try_into().unwrap(),
            sll_protocol: 0,
            sll_ifindex: self.inner.borrow().ifindex,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: libc::ETH_ALEN.try_into().unwrap(),
            sll_addr: [dest_addr_arr[0], dest_addr_arr[1], dest_addr_arr[2], dest_addr_arr[3], dest_addr_arr[4], dest_addr_arr[5], 0, 0],

        };
        let dest_addr_ll_ptr: *const libc::sockaddr_ll = &dest_addr_ll;

        let dest_addr;
        unsafe {
            let dest_addr_ptr = mem::transmute::<*const libc::sockaddr_ll, *const libc::sockaddr>(dest_addr_ll_ptr);
            dest_addr = SockAddr::from_raw_parts(dest_addr_ptr, mem::size_of::<libc::sockaddr_ll>().try_into().unwrap());
        }

        self.inner.borrow().socket.send_to(&buf, &dest_addr).unwrap();
    }

    fn receive(&self) -> Option<Bytes> {
        // I really hope recv_from never gives me more than 1 packet or I'll need to work out something clever
        let mut buf = BytesMut::zeroed(4096);
        let read_result = self.inner.borrow().socket.recv_from(&mut buf[..]);

        match read_result {
            Ok((received_size, _origin)) => Some(BytesMut::from(&buf[..received_size]).freeze()),
            Err(_) => None,
        }

        
    }

    fn scheduler(&self) -> &Scheduler<Operation<Self>> {
        &self.scheduler
    }

    fn local_link_addr(&self) -> MacAddress {
        self.inner.borrow().link_addr.clone()
    }

    fn local_ipv4_addr(&self) -> Ipv4Addr {
        self.inner.borrow().ipv4_addr.clone()
    }

    fn tcp_options(&self) -> tcp::Options {
        self.inner.borrow().tcp_options.clone()
    }

    fn arp_options(&self) -> arp::Options {
        self.inner.borrow().arp_options.clone()
    }

    fn advance_clock(&self, now: Instant) {
        self.inner.borrow_mut().timer.0.advance_clock(now);
    }

    fn wait(&self, duration: Duration) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        let now = inner.timer.0.now();
        inner
            .timer
            .0
            .wait_until(inner.timer.clone(), now + duration)
    }

    fn wait_until(&self, when: Instant) -> Self::WaitFuture {
        let inner = self.inner.borrow_mut();
        inner.timer.0.wait_until(inner.timer.clone(), when)
    }

    fn now(&self) -> Instant {
        self.inner.borrow().timer.0.now()
    }

    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>,
    {
        let mut inner = self.inner.borrow_mut();
        inner.rng.gen()
    }

    fn spawn<F: Future<Output = ()> + 'static>(&self, future: F) -> SchedulerHandle {
        self.scheduler
            .insert(Operation::Background(future.boxed_local()))
    }
}

pub fn new_mininet_engine(name: &str, now: Instant, link_addr: MacAddress, ipv4_addr: Ipv4Addr) -> MininetEngine {
    let rt = MininetRuntime::new(name, now, link_addr, ipv4_addr);
    Engine::new(rt).unwrap()
}