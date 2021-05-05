use super::{
    CongestionControl,
    Options,
    SlowStartCongestionAvoidance,
    FastRetransmitRecovery,
    LimitedTransmit,
};
use crate::protocols::tcp::SeqNumber;
use std::fmt::Debug;

// Implementation of congestion control which does nothing.
#[derive(Debug)]
pub struct None {}

impl CongestionControl for None {
    fn new(_mss: usize, _seq_no: SeqNumber, _options: Option<Options>) -> Box<dyn CongestionControl> { Box::new(Self {}) }
}

impl SlowStartCongestionAvoidance for None {}
impl FastRetransmitRecovery for None {}
impl LimitedTransmit for None {}
