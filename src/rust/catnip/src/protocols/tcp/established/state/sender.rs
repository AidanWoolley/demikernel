use super::rto::RtoCalculator;
use crate::{
    collections::watched::WatchedValue,
    fail::Fail,
    protocols::tcp::SeqNumber,
    protocols::tcp::options::TcpCongestionControlType,
    sync::Bytes,
};
use std::{
    boxed::Box,
    cell::RefCell,
    collections::VecDeque,
    convert::TryInto,
    fmt,
    num::Wrapping,
    time::{
        Duration,
        Instant,
    },
    cmp,
};

pub struct UnackedSegment {
    pub bytes: Bytes,
    // Set to `None` on retransmission to implement Karn's algorithm.
    pub initial_tx: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenderState {
    Open,
    Closed,
    SentFin,
    FinAckd,

    #[allow(unused)]
    Reset,
}

pub trait CongestionControlAlgorithm: fmt::Debug {
    fn on_dup_ack_received(&self, sender: &Sender, ack_seq_no: SeqNumber);
    fn on_ack_received(&self, sender: &Sender, ack_seq_no: SeqNumber);
    fn on_rto(&self, sender: &Sender); // Called immediately before retransmit executed
    fn on_fast_retransmit(&self, sender: &Sender);
}

#[derive(Debug)]
pub struct CongestionControl {
    pub cwnd: WatchedValue<u32>,
    pub ssthresh: WatchedValue<u32>,
    // flightSize = sent_seq_no - base_seq_no!
    // probably needn't be this big
    pub duplicate_ack_count: WatchedValue<u32>,
    pub in_fast_recovery: WatchedValue<bool>,
    pub fast_retransmit_now: WatchedValue<bool>,
    pub algorithm: Box<dyn CongestionControlAlgorithm>,
} 
 
impl CongestionControl {
    pub fn new (mss: usize, algorithm: Box<dyn CongestionControlAlgorithm>) -> Self {
        let mss: u32 = mss.try_into().unwrap();
        let initial_cwnd = match mss {
            0..=1095 => 4 * mss,
            1096..=2190 => 3 * mss,
            _ => 2 * mss
        };

        Self {
            cwnd: WatchedValue::new(initial_cwnd),
            // According to RFC5681 ssthresh should be initialised 'arbitrarily high'
            ssthresh: WatchedValue::new(u32::MAX),
            duplicate_ack_count: WatchedValue::new(0),
            in_fast_recovery: WatchedValue::new(false),
            fast_retransmit_now: WatchedValue::new(false),
            algorithm,
        }
    }
}

// Implementation of congestion control which does nothing.
#[derive(Debug)]
pub struct NoCongestionControl {}

impl CongestionControlAlgorithm for NoCongestionControl {
    fn on_dup_ack_received(&self, _sender: &Sender, _ack_seq_no: SeqNumber) {}
    fn on_ack_received(&self, _sender: &Sender, _ack_seq_no: SeqNumber) {}
    fn on_rto(&self, _sender: &Sender) {}
    fn on_fast_retransmit(&self, _sender: &Sender) {}
}

#[derive(Debug)]
pub struct Cubic {
    pub C: f32,
    pub K: f32,
    pub beta_cubic: f32,
    pub recover: WatchedValue<SeqNumber>,
    pub w_max: WatchedValue<u32>,
    pub w_last_max: WatchedValue<u32>,
}

impl CongestionControlAlgorithm for Cubic {
    fn on_dup_ack_received(&self, sender: &Sender, ack_seq_no: SeqNumber) {
        let cc = &sender.congestion_ctrl;
        cc.duplicate_ack_count.modify(|s| s + 1);
        let duplicate_ack_count = cc.duplicate_ack_count.get();
        let mss: u32 = sender.mss.try_into().unwrap();
        if duplicate_ack_count == 3 {
            // Check against recover specified in RFC6582
            if ack_seq_no - Wrapping(1) > self.recover.get() {
                cc.in_fast_recovery.set(true);
                self.recover.set(sender.sent_seq_no.get());
                let cwnd = cc.cwnd.get();
                self.w_max.set(cwnd);
                cc.ssthresh.set(((2f32).max(cwnd as f32 * self.beta_cubic) * mss as f32) as u32);
                cc.cwnd.modify(|s| (s as f32 * self.beta_cubic) as u32);
                cc.fast_retransmit_now.set(true);
            }
        } else if duplicate_ack_count > 3 || cc.in_fast_recovery.get() {
            cc.cwnd.modify(|s| s + mss);
        }
    }

    fn on_ack_received(&self, sender: &Sender, ack_seq_no: SeqNumber) {
        let cc = &sender.congestion_ctrl;
        let bytes_outstanding = sender.sent_seq_no.get() - sender.base_seq_no.get();
        let bytes_acknowledged = ack_seq_no - sender.base_seq_no.get();

        cc.duplicate_ack_count.set(0);
        if cc.in_fast_recovery.get() {
            // Fast Recovery response to new data
            let mss: u32 = sender.mss.try_into().unwrap();
            if ack_seq_no > self.recover.get() {
                // Full acknowledgement
                let ssthresh = cc.ssthresh.get();
                cc.cwnd.set(cmp::min(ssthresh, cmp::max(bytes_outstanding.0, mss) + mss));
                cc.in_fast_recovery.set(false);
            } else {
                // Partial acknowledgement
                cc.fast_retransmit_now.set(true);
                if bytes_acknowledged.0 >= mss {
                    cc.cwnd.modify(|s| s - bytes_acknowledged.0 + mss);
                } else {
                    cc.cwnd.modify(|s| s - bytes_acknowledged.0);
                }
                // We stay in fast recovery mode here because we haven't acknowledged all data up to `recovery`
            }
        }
    }

    fn on_rto(&self, sender: &Sender) {
        self.recover.set(sender.sent_seq_no.get());
        sender.congestion_ctrl.in_fast_recovery.set(false);
    }

    fn on_fast_retransmit(&self, sender: &Sender) {
        // NOTE: Could we potentially miss FastRetransmit requests with just a flag? Should we really have a count/queue?
        // I suspect it doesn't matter because we only retransmit on the 3rd repeat ACK precisely...
        sender.congestion_ctrl.fast_retransmit_now.modify_without_notify(|_| false);
    }
}

impl Cubic {
    pub fn new(seq_no: SeqNumber) -> Self {
        Self {
            C: 0.4,
            K: 0.0,
            beta_cubic: 0.7,
            recover: WatchedValue::new(seq_no),
            w_max: WatchedValue::new(0),
            w_last_max: WatchedValue::new(0),
        }
    }
}

pub struct Sender {
    pub state: WatchedValue<SenderState>,

    // TODO: Just use Figure 5 from RFC 793 here.
    //
    //                    |------------window_size------------|
    //
    //               base_seq_no               sent_seq_no           unsent_seq_no
    //                    v                         v                      v
    // ... ---------------|-------------------------|----------------------| (unavailable)
    //       acknowleged         unacknowledged     ^        unsent
    //
    pub base_seq_no: WatchedValue<SeqNumber>,
    pub unacked_queue: RefCell<VecDeque<UnackedSegment>>,
    pub sent_seq_no: WatchedValue<SeqNumber>,
    pub unsent_queue: RefCell<VecDeque<Bytes>>,
    pub unsent_seq_no: WatchedValue<SeqNumber>,

    pub window_size: WatchedValue<u32>,
    // RFC 1323: Number of bits to shift advertised window, defaults to zero.
    pub window_scale: u8,

    pub mss: usize,

    pub retransmit_deadline: WatchedValue<Option<Instant>>,
    pub rto: RefCell<RtoCalculator>,

    pub congestion_ctrl: CongestionControl,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("base_seq_no", &self.base_seq_no)
            .field("sent_seq_no", &self.sent_seq_no)
            .field("unsent_seq_no", &self.unsent_seq_no)
            .field("window_size", &self.window_size)
            .field("window_scale", &self.window_scale)
            .field("mss", &self.mss)
            .field("retransmit_deadline", &self.retransmit_deadline)
            .field("rto", &self.rto)
            .finish()
    }
}

impl Sender {
    pub fn new(seq_no: SeqNumber, window_size: u32, window_scale: u8, mss: usize, congestion_ctrl_type: TcpCongestionControlType) -> Self {
        let congestion_ctrl_alg: Box<dyn CongestionControlAlgorithm> = match congestion_ctrl_type {
            TcpCongestionControlType::None => Box::new(NoCongestionControl{}),
            TcpCongestionControlType::Cubic => Box::new(Cubic::new(seq_no)),
        }; 
        Self {
            state: WatchedValue::new(SenderState::Open),

            base_seq_no: WatchedValue::new(seq_no),
            unacked_queue: RefCell::new(VecDeque::new()),
            sent_seq_no: WatchedValue::new(seq_no),
            unsent_queue: RefCell::new(VecDeque::new()),
            unsent_seq_no: WatchedValue::new(seq_no),

            window_size: WatchedValue::new(window_size),
            window_scale,
            mss,

            retransmit_deadline: WatchedValue::new(None),
            rto: RefCell::new(RtoCalculator::new()),

            congestion_ctrl: CongestionControl::new(mss, congestion_ctrl_alg),
        }
    }

    pub fn send<RT: crate::runtime::Runtime>(&self, buf: Bytes, cb: &super::ControlBlock<RT>) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Sender closed",
            });
        }
        let buf_len: u32 = buf.len().try_into().map_err(|_| Fail::Ignored {
            details: "Buffer too large",
        })?;

        let win_sz = self.window_size.get();
        let base_seq = self.base_seq_no.get();
        let sent_seq = self.sent_seq_no.get();
        let cwnd = self.congestion_ctrl.cwnd.get();
        let Wrapping(sent_data) = sent_seq - base_seq;

        // Fast path: Try to send the data immediately.
        if win_sz > 0 && win_sz >= sent_data + buf_len && cwnd >= sent_data + buf_len {
            if let Some(remote_link_addr) = cb.arp.try_query(cb.remote.address()) {
                let mut header = cb.tcp_header();
                header.seq_num = sent_seq;
                cb.emit(header, buf.clone(), remote_link_addr);

                self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));
                self.sent_seq_no.modify(|s| s + Wrapping(buf_len));
                let unacked_segment = UnackedSegment {
                    bytes: buf,
                    initial_tx: Some(cb.rt.now()),
                };
                self.unacked_queue.borrow_mut().push_back(unacked_segment);
                if self.retransmit_deadline.get().is_none() {
                    let rto = self.rto.borrow().estimate();
                    self.retransmit_deadline.set(Some(cb.rt.now() + rto));
                }
                return Ok(());
            }
        }
        // Slow path: Delegating sending the data to background processing.
        self.unsent_queue.borrow_mut().push_back(buf);
        self.unsent_seq_no.modify(|s| s + Wrapping(buf_len));

        Ok(())
    }

    pub fn close(&self) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Sender closed",
            });
        }
        self.state.set(SenderState::Closed);
        Ok(())
    }

    pub fn remote_ack(&self, ack_seq_no: SeqNumber, now: Instant) -> Result<(), Fail> {
        if self.state.get() == SenderState::SentFin {
            assert_eq!(self.base_seq_no.get(), self.sent_seq_no.get());
            assert_eq!(self.sent_seq_no.get(), self.unsent_seq_no.get());
            self.state.set(SenderState::FinAckd);
            return Ok(());
        }

        let base_seq_no = self.base_seq_no.get();
        let sent_seq_no = self.sent_seq_no.get();

        let bytes_outstanding = sent_seq_no - base_seq_no;
        let bytes_acknowledged = ack_seq_no - base_seq_no;

        if bytes_acknowledged > bytes_outstanding {
            return Err(Fail::Ignored {
                details: "ACK is outside of send window",
            });
        }

        if bytes_acknowledged.0 == 0 {
            // Duplicate ACK
            // TODO: Congestion control
            // TODO: Handle fast retransmit here.
            self.congestion_ctrl.algorithm.on_dup_ack_received(&self, ack_seq_no);
            return Ok(());
        } else {
            self.congestion_ctrl.algorithm.on_ack_received(&self, ack_seq_no);
        }

        if ack_seq_no == sent_seq_no {
            // If we've acknowledged all sent data, turn off the retransmit timer.
            self.retransmit_deadline.set(None);
        } else {
            // Otherwise, set it to the current RTO.
            let deadline = now + self.rto.borrow().estimate();
            self.retransmit_deadline.set(Some(deadline));
        }

        // TODO: Do acks need to be on segment boundaries? How does this interact with repacketization?
        let mut bytes_remaining = bytes_acknowledged.0 as usize;
        while let Some(segment) = self.unacked_queue.borrow_mut().pop_front() {
            if segment.bytes.len() > bytes_remaining {
                // TODO: We need to close the connection in this case.
                return Err(Fail::Ignored {
                    details: "ACK isn't on segment boundary",
                });
            }
            bytes_remaining -= segment.bytes.len();

            // Add sample for RTO if not a retransmission
            // TODO: TCP timestamp support.
            if let Some(initial_tx) = segment.initial_tx {
                self.rto.borrow_mut().add_sample(now - initial_tx);
            }
            if bytes_remaining == 0 {
                break;
            }
        }
        // TODO: Congestion control
        // TODO: Modify cwnd for slow start, congestion avoidance or fast recovery

        self.base_seq_no.modify(|b| b + bytes_acknowledged);

        Ok(())
    }

    pub fn pop_one_unsent_byte(&self) -> Option<Bytes> {
        let mut queue = self.unsent_queue.borrow_mut();
        let buf = queue.pop_front()?;
        let (byte, remainder) = buf.split(1);
        queue.push_front(remainder);
        Some(byte)
    }

    pub fn pop_unsent(&self, max_bytes: usize) -> Option<Bytes> {
        // TODO: Use a scatter/gather array to coalesce multiple buffers into a single segment.
        let mut unsent_queue = self.unsent_queue.borrow_mut();
        let mut buf = unsent_queue.pop_front()?;
        if buf.len() > max_bytes {
            let (head, tail) = buf.split(max_bytes);
            buf = head;
            unsent_queue.push_front(tail);
        }
        Some(buf)
    }

    pub fn update_remote_window(&self, window_size_hdr: u16) -> Result<(), Fail> {
        if self.state.get() != SenderState::Open {
            return Err(Fail::Ignored {
                details: "Dropping remote window update for closed sender",
            });
        }

        // TODO: Is this the right check?
        let window_size = (window_size_hdr as u32)
            .checked_shl(self.window_scale as u32)
            .ok_or_else(|| Fail::Ignored {
                details: "Window size overflow",
            })?;
        self.window_size.set(window_size);

        Ok(())
    }

    pub fn remote_mss(&self) -> usize {
        self.mss
    }

    pub fn current_rto(&self) -> Duration {
        self.rto.borrow().estimate()
    }
}
