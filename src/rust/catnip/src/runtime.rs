// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use crate::{
    protocols::{
        arp,
        ethernet2::MacAddress,
        tcp,
    },
    scheduler::{
        Operation,
        Scheduler,
        SchedulerHandle,
    },
};
use rand::distributions::{
    Distribution,
    Standard,
};
use std::{
    fmt,
    ptr,
    ops::{Deref, DerefMut},
    future::Future,
    net::Ipv4Addr,
    time::{
        Duration,
        Instant,
    },
};

pub const PKTBUF_SIZE: usize = 1514;

pub struct ManagedPacketBuf<RT: Runtime> {
    buf: Option<PacketBuf>,
    rt: RT,
}

impl<RT: Runtime> Deref for ManagedPacketBuf<RT> {
    type Target = PacketBuf;

    fn deref(&self) -> &PacketBuf {
        self.buf.as_ref().unwrap()
    }
}

impl<RT: Runtime> DerefMut for ManagedPacketBuf<RT> {
    fn deref_mut(&mut self) -> &mut PacketBuf {
        self.buf.as_mut().unwrap()
    }
}

impl<RT: Runtime> ManagedPacketBuf<RT> {
    pub fn new(rt: RT, buf: PacketBuf) -> Self {
        Self { rt, buf: Some(buf) }
    }

    pub fn split(mut self, ix: usize) -> Self {
        let buf = self.buf.take().unwrap();
        Self { rt: self.rt.clone(), buf: Some(buf.split(ix)) }
    }

    pub fn take(mut self) -> PacketBuf {
        self.buf.take().unwrap()
    }
}

impl<RT: Runtime> Drop for ManagedPacketBuf<RT> {
    fn drop(&mut self) {
        if let Some(buf) = self.buf.take() {
            self.rt.free_pktbuf(buf)
        }
    }
}

pub struct PacketBuf {
    buf: Option<Box<[u8; PKTBUF_SIZE]>>,
    offset: usize,
    len: usize,
}

impl PacketBuf {
    pub fn alloc() -> Self {
        Self::new(unsafe { Box::new_zeroed().assume_init() })
    }

    pub fn new(buf: Box<[u8; PKTBUF_SIZE]>) -> Self {
        PacketBuf { len: buf.len(), buf: Some(buf), offset: 0 }
    }

    pub fn into_buf(self) -> Option<Box<[u8; PKTBUF_SIZE]>> {
        self.buf
    }

    pub const fn empty() -> Self {
        Self {
            buf: None,
            offset: 0,
            len: 0,
        }
    }

    pub fn split(self, ix: usize) -> Self {
        if self.buf.is_none() && ix == 0 {
            return self;
        }
        let buf = self.buf.unwrap();
        assert!(ix < self.len);
        Self {
            buf: Some(buf),
            offset: self.offset + ix,
            len: self.len - ix,
        }
    }

    pub fn trim(mut self, new_len: usize) -> Self {
        assert!(new_len < self.len);
        self.len = new_len;
        self
    }

    pub fn unsplit_left(mut self, hdr_bytes: usize) -> Option<Self> {
        if hdr_bytes > self.offset {
            return None;
        }
        self.offset -= hdr_bytes;
        self.len += hdr_bytes;
        Some(self)
    }

    pub fn unsplit_right(mut self, padding_bytes: usize) -> Option<Self> {
        if self.buf.is_none() {
            return None;
        }
        if self.offset + self.len + padding_bytes > PKTBUF_SIZE {
            return None;
        }
        self.len += padding_bytes;
        Some(self)
    }

    pub fn into_raw(self) -> *mut libc::c_void {
        self.buf.map(Box::into_raw).unwrap_or_else(ptr::null_mut).cast()
    }

    pub fn from_raw(ptr: *mut libc::c_void, slice_ptr: *mut libc::c_void, slice_len: usize) -> Self {
        if ptr.is_null() {
            assert_eq!(slice_len, 0);
            Self::empty()
        } else {
            let offset = unsafe { slice_ptr.cast::<u8>().offset_from(ptr.cast::<u8>()) };
            assert!(offset > 0);
            Self {
                buf: Some(unsafe { Box::from_raw(ptr.cast()) }),
                offset: offset as usize,
                len: slice_len,
            }
        }
    }
}

impl fmt::Debug for PacketBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PacketBuf({:?})", &self[..])
    }
}

impl PartialEq for PacketBuf {
    fn eq(&self, rhs: &Self) -> bool {
        &self[..] == &rhs[..]
    }
}

impl Eq for PacketBuf {}

impl Deref for PacketBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self.buf {
            None => &[],
            Some(ref buf) => &buf[self.offset..(self.offset + self.len)],
        }
    }
}

impl DerefMut for PacketBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self.buf {
            None => &mut [],
            Some(ref mut buf) => &mut buf[self.offset..(self.offset + self.len)],
        }
    }
}


pub trait PacketSerialize: Sized {
    fn compute_size(&self) -> usize;
    fn serialize(&self, buf: &mut [u8]);
    fn serialize2(self) -> PacketBuf {
        todo!();
    }
}

pub trait Runtime: Clone + Unpin + 'static {
    fn advance_clock(&self, now: Instant);
    fn transmit(&self, pkt: impl PacketSerialize);
    fn receive(&self) -> Option<PacketBuf>;

    fn local_link_addr(&self) -> MacAddress;
    fn local_ipv4_addr(&self) -> Ipv4Addr;
    fn arp_options(&self) -> arp::Options;
    fn tcp_options(&self) -> tcp::Options;

    type WaitFuture: Future<Output = ()>;
    fn wait(&self, duration: Duration) -> Self::WaitFuture;
    fn wait_until(&self, when: Instant) -> Self::WaitFuture;
    fn now(&self) -> Instant;

    fn rng_gen<T>(&self) -> T
    where
        Standard: Distribution<T>;

    fn spawn<F: Future<Output = ()> + 'static>(&self, future: F) -> SchedulerHandle;
    fn scheduler(&self) -> &Scheduler<Operation<Self>>;

    fn alloc_pktbuf(&self) -> PacketBuf;
    fn free_pktbuf(&self, buf: PacketBuf);
}
