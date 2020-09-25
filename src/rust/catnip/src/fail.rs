// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use custom_error::custom_error;
use float_duration;
use std::{
    cell::BorrowMutError, io::Error as IoError,
    num::TryFromIntError,
};

// the following type alias is needed because the `custom_error!` macro doesn't
// allow `&` in type specifications.
type Str = &'static str;

custom_error! {#[derive(Clone)] pub Fail
    ConnectionAborted{} = "connection aborted",
    ConnectionRefused{} = "connection refused",
    IoError {} = "IO Error",
    BorrowMutError {} = "BorrowMut Error",
    Ignored{details: Str} = "operation had no effect ({details})",
    Malformed{details: Str} = "encountered a malformed datagram ({details})",
    Misdelivered{} = "misdelivered datagram",
    OutOfRange{details: Str} = "a value is out of range ({details})",
    ResourceBusy{details: Str} = "resource is busy ({details})",
    ResourceExhausted{details: Str} = "resource exhausted ({details})",
    ResourceNotFound{details: Str} = "resource not found ({details})",
    Timeout{} = "an asynchronous operation timed out",
    TypeMismatch{details: Str} = "type mismatch ({details})",
    Unsupported{details: Str} = "unsupported ({details})",
}

impl From<IoError> for Fail {
    fn from(_: IoError) -> Self {
        Fail::IoError {}
    }
}

impl From<BorrowMutError> for Fail {
    fn from(_: BorrowMutError) -> Self {
        Fail::BorrowMutError {}
    }
}

impl From<float_duration::error::OutOfRangeError> for Fail {
    fn from(_: float_duration::error::OutOfRangeError) -> Self {
        Fail::OutOfRange {
            details: "float_duration::error::OutOfRangeError",
        }
    }
}

impl From<TryFromIntError> for Fail {
    fn from(_: TryFromIntError) -> Self {
        Fail::OutOfRange {
            details: "std::num::TryFromIntError",
        }
    }
}
