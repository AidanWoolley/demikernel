use super::sender::Sender;
use crate::{
    collections::watched::WatchFuture,
    protocols::tcp::SeqNumber,
};
use std::fmt::Debug;

mod cubic;
mod none;
mod options;
pub use self::{
    cubic::Cubic,
    none::None,
    options::{
        Options,
        OptionValue,
    },
};

pub trait SlowStartCongestionAvoidance { 
    fn get_cwnd(&self) -> u32 { u32::MAX }
    fn watch_cwnd(&self) -> (u32,  WatchFuture<'_, u32>) { (u32::MAX, WatchFuture::Pending) }

    // Called immediately before the cwnd check is performed before data is sent
    fn on_cwnd_check_before_send(&self, _sender: &Sender) {}

    fn on_ack_received(&self, _sender: &Sender, _ack_seq_no: SeqNumber) {}
    
    // Called immediately before retransmit after RTO
    fn on_rto(&self, _sender: &Sender) {}

    // Called immediately before a segment is sent for the 1st time
    fn on_send(&self, _sender: &Sender, _num_sent_bytes: u32) {}
}

pub trait FastRetransmitRecovery where Self: SlowStartCongestionAvoidance {
    fn get_duplicate_ack_count(&self) -> u32 { 0 }

    fn get_retransmit_now_flag(&self) -> bool { false }
    fn watch_retransmit_now_flag(&self) -> (bool, WatchFuture<'_, bool>) { (false, WatchFuture::Pending) }

    fn on_fast_retransmit(&self, _sender: &Sender) {}
    fn on_base_seq_no_wraparound(&self, _sender: &Sender) {}
}

pub trait LimitedTransmit where Self: SlowStartCongestionAvoidance {
    fn get_limited_transmit_cwnd_increase(&self) -> u32 { 0 }
    fn watch_limited_transmit_cwnd_increase(&self) -> (u32, WatchFuture<'_, u32>) {(0, WatchFuture::Pending) }
} 


pub trait CongestionControl: SlowStartCongestionAvoidance +
                             FastRetransmitRecovery +
                             LimitedTransmit +
                             Debug {
    fn new(mss: usize, seq_no: SeqNumber, options: Option<options::Options>) -> Box<dyn CongestionControl> where Self: Sized;
}

pub type CongestionControlConstructor = fn(usize, SeqNumber, Option<options::Options>) -> Box<dyn CongestionControl>;
