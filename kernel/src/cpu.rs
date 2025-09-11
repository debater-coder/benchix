use core::cell::UnsafeCell;

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::instructions::interrupts::enable_and_hlt;
use x86_64::instructions::segmentation::Segment;
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::registers::control::{Efer, EferFlags};
use x86_64::registers::model_specific::{LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;

use crate::scheduler::Thread;
use crate::user::syscalls::handle_syscall;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Per-CPU data
/// In future, each process will have its own kernel stack
/// For simplicity, we handle interrupts on the kernel stack of the current stack
/// The linux kernel has a separate stack for this to save stack space.
/// That's why we keep the TSS in an UnsafeCell, so we can update the interrupt handling stack.
pub struct PerCpu {
    pub gdt: GlobalDescriptorTable,
    tss: &'static mut TaskStateSegment,
    pub current_thread: Option<Arc<Mutex<Thread>>>,
    pub next_thread: Option<Arc<Mutex<Thread>>>,
    pub idle_thread: Arc<Mutex<Thread>>,
}

impl PerCpu {
    /// Initialises a CPU
    pub unsafe fn init_cpu() -> Self {
        let tss = Box::leak(Box::new(TaskStateSegment::new()));
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            #[allow(unused_unsafe)]
            let stack_start = VirtAddr::from_ptr(unsafe { &raw const STACK });
            let stack_end = stack_start + STACK_SIZE as u64;

            stack_end // stacks grow downwards
        };

        // Setting up gdt
        let gdt = GlobalDescriptorTable::new();

        PerCpu {
            gdt,
            tss,
            current_thread: None,
            next_thread: None,
            idle_thread: Arc::new(Mutex::new(Thread::from_func(
                idle,
                None,
                Some("idle".to_owned()),
            ))),
        }
    }

    pub unsafe fn set_ist(&mut self, top: VirtAddr) {
        self.tss.privilege_stack_table[0] = top;
    }

    pub unsafe fn init_gdt(&'static mut self) {
        // Intel manual vol 3 3.4.2: A segment selector is a 16-bit identifier for a segment (see Figure 3-6). It does not point directly to the segment, // but instead points to the segment descriptor that defines the segment.
        let code_selector = self.gdt.append(Descriptor::kernel_code_segment());
        let data_selector = self.gdt.append(Descriptor::kernel_data_segment());
        let tss_selector = self.gdt.append(Descriptor::tss_segment(&self.tss));
        let user_data_selector = self.gdt.append(Descriptor::user_data_segment());
        let user_code_selector = self.gdt.append(Descriptor::user_code_segment());

        self.gdt.load();

        unsafe {
            CS::set_reg(code_selector);
            load_tss(tss_selector);

            DS::set_reg(data_selector);
            ES::set_reg(data_selector);
            FS::set_reg(data_selector);
            GS::set_reg(data_selector);
            SS::set_reg(data_selector);

            // Prepare for usermode
            Efer::write(Efer::read() | EferFlags::SYSTEM_CALL_EXTENSIONS);
        }
        Star::write(
            user_code_selector,
            user_data_selector,
            code_selector,
            data_selector,
        )
        .unwrap();
        LStar::write(VirtAddr::from_ptr(handle_syscall as *const ()));
        SFMask::write(RFlags::INTERRUPT_FLAG);
    }
}

/// A Send + Sync structure storing all the per CPU data. We ensure CPUs can only access their own data, preventing data races.
/// Eventually this will have an array indexed by LAPIC ID.
/// TODO: make a `WithoutInterruptsCell`
pub struct Cpus {
    cpu: UnsafeCell<PerCpu>, // Only have one CPU right now
}

impl Cpus {
    pub fn new(current_cpu: PerCpu) -> Self {
        Cpus {
            cpu: UnsafeCell::new(current_cpu),
        }
    }

    pub fn get_cpu(&self) -> &mut PerCpu {
        unsafe { self.cpu.get().as_mut().unwrap() }
    }
}

unsafe impl Send for Cpus {}
unsafe impl Sync for Cpus {}

extern "sysv64" fn idle() {
    loop {
        enable_and_hlt();
    }
}
