use super::rto::RtoCalculator;
use crate::{
    collections::watched::WatchedValue,
    fail::Fail,
    protocols::tcp::SeqNumber,
    sync::Bytes,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::TryInto,
    fmt,
    num::Wrapping,
    time::{
        Duration,
        Instant,
    },
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

pub struct CongestionControlState<AlgParams> {
    pub cwnd: WatchedValue<u32>,
    pub ssthresh: WatchedValue<u32>,
    // flightSize = sent_seq_no - base_seq_no!
    // probably needn't be this big
    pub repeat_ack_count: WatchedValue<u32>,
    pub params: AlgParams
}

impl<AlgParams: fmt::Debug> fmt::Debug for CongestionControlState<AlgParams> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("CongestionControlState")
            .field("cwnd", &self.cwnd)
            .field("ssthresh", &self.ssthresh)
            .field("params", &self.params)
            .finish()
    }
}

impl<AlgParams> CongestionControlState<AlgParams> {
    pub fn new(mss: usize, params: AlgParams) -> Self {
        let mss: u32 = mss.try_into().unwrap();
        let initial_cwnd = match mss {
            0..=1095 => 4 * mss,
            1096..=2190 => 3 * mss,
            _ => 2 * mss
        };
        Self {
            cwnd: WatchedValue::new(initial_cwnd),
            ssthresh: WatchedValue::new(u32::MAX),
            repeat_ack_count: WatchedValue::new(0),
            params,
        }
    }
}

pub struct CubicParams {
    pub C: f32,
    pub K: f32,
    pub beta_cubic: f32,
    pub recover: WatchedValue<u32>,
    pub W_max: usize,
    pub W_last_max: usize,
}

impl CubicParams {
    pub fn new(seq_no: SeqNumber) -> Self {
        Self {
            C: 0.4,
            K: 0.0,
            beta_cubic: 0.7,
            recover: WatchedValue::new(seq_no.0),
            W_max: 0,
            W_last_max: 0,
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

    congestion_ctrl_state: CongestionControlState<CubicParams>,
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
    pub fn new(seq_no: SeqNumber, window_size: u32, window_scale: u8, mss: usize) -> Self {
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

            congestion_ctrl_state: CongestionControlState::<CubicParams>::new(mss, CubicParams::new(seq_no))
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
        let cwnd = self.congestion_ctrl_state.cwnd.get();
        let Wrapping(sent_data) = sent_seq - base_seq;

        // Fast path: Try to send the data immediately.
        // TODO: Congestion control
        // TODO: Edge case when cwnd wraps before sequence number: might we get stuck?!
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
            // TODO: Congestion control
            // TODO: Handle fast retransmit here.
            return Ok(());
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
