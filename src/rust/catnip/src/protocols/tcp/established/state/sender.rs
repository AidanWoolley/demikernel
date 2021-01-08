use super::{
    rto::RtoCalculator,
    congestion_control::{
        CongestionControl,
        NoCongestionControl,
        Cubic,
        CongestionControlOptions,
    }
};
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

pub struct Sender {
    pub state: WatchedValue<SenderState>,

    // TODO: Just use Figure 5 from RFC 793 here.
    //
    //                    |------------window_size------------|
    //
    //               base_seq_no               sent_seq_no           unsent_seq_no
    //                    v                         v                      v
    // ... ---------------|-------------------------|----------------------| (unavailable)
    //       acknowledged        unacknowledged     ^        unsent
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

    pub congestion_ctrl: Box<dyn CongestionControl>,
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
    pub fn new(seq_no: SeqNumber, window_size: u32, window_scale: u8, mss: usize, congestion_ctrl_type: TcpCongestionControlType, congestion_control_options: Option<CongestionControlOptions>) -> Self {
        let congestion_ctrl: Box<dyn CongestionControl> = match congestion_ctrl_type {
            TcpCongestionControlType::None => Box::new(NoCongestionControl::new(mss, seq_no, congestion_control_options)),
            TcpCongestionControlType::Cubic => Box::new(Cubic::new(mss, seq_no, congestion_control_options)),
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

            congestion_ctrl,
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
        let Wrapping(sent_data) = sent_seq - base_seq;
        
        // Fast path: Try to send the data immediately.
        let in_flight_after_send = sent_data + buf_len;

        // Before we get cwnd for the check, we prompt it to shrink it if the connection has been idle
        self.congestion_ctrl.on_cwnd_check_before_send(&self);
        let cwnd = self.congestion_ctrl.get_cwnd();
        // The limited transmit algorithm can increase the effective size of cwnd by up to 2MSS
        let effective_cwnd = cwnd + self.congestion_ctrl.get_limited_transmit_cwnd_increase();

        if win_sz > 0 && win_sz >= in_flight_after_send && effective_cwnd >= in_flight_after_send {
            if let Some(remote_link_addr) = cb.arp.try_query(cb.remote.address()) {
                // This hook is primarily intended to record the last time we sent data, so we can later tell if the connection has been idle
                self.congestion_ctrl.on_send(&self);

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

        self.congestion_ctrl.on_ack_received(&self, ack_seq_no);
        if bytes_acknowledged.0 == 0 {
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
        self.base_seq_no.modify(|b| b + bytes_acknowledged);
        let new_base_seq_no = self.base_seq_no.get();
        if new_base_seq_no < base_seq_no {
            // We've wrapped around, and so we need to do some bookkeeping
            self.congestion_ctrl.on_base_seq_no_wraparound(&self);
        }

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
