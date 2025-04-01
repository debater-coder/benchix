use core::ptr::slice_from_raw_parts_mut;

use x86_64::{
    structures::paging::{Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{debug_println, memory::PhysicalMemoryManager, LAPIC_START_VIRT};

pub const LAPIC_ID_OFFSET: u64 = 0x20;
pub const SIVR_OFFSET: u64 = 0xf0;
pub const DESTINATION_FORMAT_OFFSET: u64 = 0xe0;
pub const TASK_PRIORITY_OFFSET: u64 = 0x80;
pub const INITIAL_COUNT_REGISTER_OFFSET: u64 = 0x380;
pub const LVT_TIMER_OFFSET: u64 = 0x320;
pub const DIVIDE_CONFIG_OFFSET: u64 = 0x3e0;

pub const EOI_OFFSET: u64 = 0xB0;
pub const LAPIC_BASE_PHYSICAL_ADDRESS: u64 = 0xFEE0_0000;

pub unsafe fn lapic_end_of_interrupt() {
    (VirtAddr::new(LAPIC_START_VIRT as u64 + EOI_OFFSET).as_mut_ptr() as *mut u32).write(0);
}

#[allow(dead_code)]
pub enum TimerDivideConfig {
    DivideBy2 = 0b0000,
    DivideBy4 = 0b0001,
    DivideBy8 = 0b0010,
    DivideBy16 = 0b0011,
    DivideBy32 = 0b1000,
    DivideBy64 = 0b1001,
    DivideBy128 = 0b1010,
    DivideBy1 = 0b1011,
}

pub struct Lapic {
    mm_region: &'static mut [u32],
}

impl Lapic {
    pub fn lapic_id(&self) -> u8 {
        ((self.read(LAPIC_ID_OFFSET)) >> 24) as u8
    }

    /// Can only be called once
    pub unsafe fn new(
        mapper: &mut OffsetPageTable<'static>,
        frame_allocator: &mut PhysicalMemoryManager,
        spurious_interrupt_vector: u8,
    ) -> Self {
        let virt_addr = VirtAddr::new(LAPIC_START_VIRT as u64);

        unsafe {
            mapper
                .map_to(
                    Page::containing_address(virt_addr) as Page<Size4KiB>,
                    PhysFrame::containing_address(PhysAddr::new(LAPIC_BASE_PHYSICAL_ADDRESS)),
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE,
                    frame_allocator,
                )
                .unwrap();
        }

        let mm_region = slice_from_raw_parts_mut(virt_addr.as_mut_ptr(), 0x1000);
        let mm_region = unsafe { &mut *mm_region };

        let mut apic = Lapic { mm_region };

        // https://forum.osdev.org/viewtopic.php?f=1&t=12045&hilit=APIC+init

        apic.write(SIVR_OFFSET, 0x100 | (spurious_interrupt_vector as u32)); // 0x100 sets bit 8 to enable APIC

        // set destination format register to flat mode
        apic.write(DESTINATION_FORMAT_OFFSET, 0xFFFFFFFF);

        // set task priority to accept all interrupts
        apic.write(TASK_PRIORITY_OFFSET, 0);

        apic
    }

    pub fn configure_timer(
        &mut self,
        vector: u8,
        timer_initial: u32,
        timer_divide: TimerDivideConfig,
    ) {
        // The order is important DO NOT CHANGE
        self.write(DIVIDE_CONFIG_OFFSET, timer_divide as u32);
        self.write(LVT_TIMER_OFFSET, (1 << 17) | (vector as u32));
        self.write(INITIAL_COUNT_REGISTER_OFFSET, timer_initial);
    }

    #[allow(dead_code)]
    fn read(&self, offset: u64) -> u32 {
        self.mm_region[offset as usize / 4]
    }
    fn write(&mut self, offset: u64, val: u32) {
        self.mm_region[offset as usize / 4] = val;
    }
}
