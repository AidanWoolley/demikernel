use super::{
    CongestionControl,
    Options,
    SlowStartCongestionAvoidance,
    FastRetransmitRecovery,
    LimitedTransmit,
};
use crate::{
    collections::watched::{WatchedValue, WatchFuture},
    protocols::tcp::SeqNumber,
};
use std::fmt::Debug;

// Implementation of congestion control which does nothing.
#[derive(Debug)]
pub struct None {
    // TODO: CongestionControl      Priority: Low
    // Work out how to construct a WatchFuture which is always pending so I can get rid of these values 
    cwnd: WatchedValue<u32>,
    retransmit_now: WatchedValue<bool>,
    limited_transmit_cwnd_increase: WatchedValue<u32>,
}

impl CongestionControl for None {
    fn new(_mss: usize, _seq_no: SeqNumber, _options: Option<Options>) -> Self {
        Self {
            cwnd: WatchedValue::new(u32::MAX),
            retransmit_now: WatchedValue::new(false),
            limited_transmit_cwnd_increase: WatchedValue::new(0),
        }
    }
}

impl SlowStartCongestionAvoidance for None {
    fn watch_cwnd(&self) -> (u32, WatchFuture<'_, u32>) { self.cwnd.watch() }
}

impl FastRetransmitRecovery for None {
    fn watch_retransmit_now_flag(&self) -> (bool, WatchFuture<'_, bool>) { self.retransmit_now.watch() }
}

impl LimitedTransmit for None {
    fn watch_limited_transmit_cwnd_increase(&self) -> (u32, WatchFuture<'_, u32>) { self.limited_transmit_cwnd_increase.watch() }
}
