use std::sync::{Arc, Mutex};

use benchix_tracing::{MAX_MSG_SIZE, TraceEvent, TraceType};
use ouroboros::self_referencing;
use uuid::Uuid;

pub struct Traces {
    pub events: Arc<Mutex<Vec<Arc<TraceOwned>>>>,
}

impl Traces {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(vec![])),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TraceFilters {
    pub syscall_begin: bool,
    pub syscall_end: bool,
    pub frame_allocate: bool,
    pub frame_deallocate: bool,
    pub mark_phys_range: bool,
    pub mark_unusable_range: bool,
    pub heap_allocate: bool,
    pub heap_deallocate: bool,
    pub msg: bool,
}

impl Default for TraceFilters {
    fn default() -> Self {
        Self {
            syscall_begin: true,
            syscall_end: true,
            frame_allocate: true,
            frame_deallocate: true,
            mark_phys_range: true,
            mark_unusable_range: true,
            heap_allocate: true,
            heap_deallocate: true,
            msg: true,
        }
    }
}

impl TraceFilters {
    pub fn filter(&self, event: &TraceEvent) -> bool {
        match event.trace_type {
            TraceType::SyscallBegin { syscall: _ } if self.syscall_begin => true,
            TraceType::SyscallEnd { syscall: _ } if self.syscall_end => true,
            TraceType::FrameAllocate(_) if self.frame_allocate => true,
            TraceType::FrameDeallocate(_) if self.frame_deallocate => true,
            TraceType::MarkPhysRange(_, _) if self.mark_phys_range => true,
            TraceType::MarkUnusableRange(_, _) if self.mark_unusable_range => true,
            TraceType::HeapAllocate { base: _, len: _ } if self.heap_allocate => true,
            TraceType::HeapDeallocate { base: _, len: _ } if self.heap_deallocate => true,
            TraceType::Msg(_) if self.msg => true,
            _ => false,
        }
    }
}

#[self_referencing(pub_extras)]
pub struct TraceOwned {
    pub id: Uuid,
    pub buf: [u8; MAX_MSG_SIZE],
    #[borrows(buf)]
    #[covariant]
    pub event: TraceEvent<'this>,
}

impl Eq for TraceOwned {}

impl PartialEq for TraceOwned {
    fn eq(&self, other: &Self) -> bool {
        self.borrow_id() == other.borrow_id()
    }
}
