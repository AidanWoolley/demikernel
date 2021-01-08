// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use crate::protocols::tcp::{
    constants::{
        DEFAULT_MSS,
        MAX_MSS,
        MIN_MSS,
    },
    established::state::congestion_ctrl as cc,
};
use std::time::Duration;



#[derive(Clone, Debug)]
pub struct TcpOptions {
    pub advertised_mss: usize,
    pub congestion_ctrl_type: cc::Type,
    pub congestion_ctrl_options: Option<cc::Options>,
    pub handshake_retries: usize,
    pub handshake_timeout: Duration,
    pub receive_window_size: usize,
    pub retries: usize,
    pub trailing_ack_delay: Duration,
}

impl Default for TcpOptions {
    fn default() -> Self {
        TcpOptions {
            advertised_mss: DEFAULT_MSS,
            congestion_ctrl_type: cc::Type::Cubic,
            congestion_ctrl_options: None,
            handshake_retries: 5,
            handshake_timeout: Duration::from_secs(3),
            receive_window_size: 0xffff,
            retries: 5,
            trailing_ack_delay: Duration::from_micros(1),
        }
    }
}

impl TcpOptions {
    pub fn advertised_mss(mut self, value: usize) -> Self {
        assert!(value >= MIN_MSS);
        assert!(value <= MAX_MSS);
        self.advertised_mss = value;
        self
    }

    pub fn congestion_ctrl_type(mut self, value: cc::Type) -> Self {
        self.congestion_ctrl_type = value;
        self
    }

    pub fn congestion_control_options(mut self, value: cc::Options) -> Self {
        self.congestion_ctrl_options = Some(value);
        self
    }

    pub fn handshake_retries(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.handshake_retries = value;
        self
    }

    pub fn handshake_timeout(mut self, value: Duration) -> Self {
        assert!(value > Duration::new(0, 0));
        self.handshake_timeout = value;
        self
    }

    pub fn receive_window_size(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.receive_window_size = value;
        self
    }

    pub fn retries(mut self, value: usize) -> Self {
        assert!(value > 0);
        self.retries = value;
        self
    }

    pub fn trailing_ack_delay(mut self, value: Duration) -> Self {
        self.trailing_ack_delay = value;
        self
    }
}
