#![no_std]
#![warn(clippy::all, clippy::pedantic)]

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy)]
pub struct TraceEvent<'a> {
    pub calling_address: u64,
    #[serde(borrow)]
    pub trace_type: TraceType<'a>,
    /// From `rdtsc`. Implementations need not insert memory barriers before this, it is only an approximate measure
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Copy)]
pub enum TraceType<'a> {
    SyscallBegin { syscall: u64 },
    SyscallEnd { syscall: u64 },
    FrameAllocate(u64),
    MarkPhysRange(u64, u64),
    MarkUnusableRange(u64, u64),
    FrameDeallocate(u64),
    HeapAllocate { base: u64, len: u64 },
    HeapDeallocate { base: u64, len: u64 },
    Msg(&'a str),
}

pub const MAX_MSG_SIZE: usize = 4 * 1024;
