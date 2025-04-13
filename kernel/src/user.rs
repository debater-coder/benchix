use core::{arch::asm, slice};

use x86_64::{
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{debug_println, memory::PhysicalMemoryManager};

unsafe fn allocate_user_page(
    mapper: &mut OffsetPageTable,
    pmm: &mut PhysicalMemoryManager,
    page: Page,
    flags: PageTableFlags,
) {
    mapper
        .map_to(
            page,
            pmm.allocate_frame().expect("Could not allocate frame"),
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE
                | flags,
            pmm,
        )
        .unwrap()
        .flush();
}

pub struct UserProcess {
    pub text: VirtAddr,
    pub stack: VirtAddr, // Top of user stack
}

impl UserProcess {
    pub unsafe fn new(
        mapper: &mut OffsetPageTable<'_>,
        pmm: &mut PhysicalMemoryManager,
        text_addr: VirtAddr,
        text_content: &[u8],
        stack_addr: VirtAddr,
        stack_content: &[u8],
    ) -> Self {
        let text_range = Page::range_inclusive(
            Page::<Size4KiB>::containing_address(text_addr),
            Page::<Size4KiB>::containing_address(text_addr + (text_content.len() - 1) as u64),
        );

        let stack_end = stack_addr;
        let stack_start = stack_addr - stack_content.len() as u64 + 1;
        let stack_range = Page::range_inclusive(
            Page::<Size4KiB>::containing_address(stack_start),
            Page::<Size4KiB>::containing_address(stack_end),
        );
        unsafe {
            for page in text_range {
                allocate_user_page(mapper, pmm, page, PageTableFlags::empty());
            }

            for page in stack_range {
                allocate_user_page(mapper, pmm, page, PageTableFlags::NO_EXECUTE);
            }

            slice::from_raw_parts_mut(text_addr.as_mut_ptr::<u8>(), text_content.len())
                .copy_from_slice(text_content);
            slice::from_raw_parts_mut(stack_end.as_mut_ptr::<u8>(), stack_content.len())
                .copy_from_slice(stack_content);
        }

        UserProcess {
            text: text_addr,
            stack: stack_end,
        }
    }

    pub fn switch(&self) {
        unsafe {
            x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
            asm!(
                "mov rsp, {}", // Stacks grow downwards
                "mov r11, 0x0202",             // Bit 9 is set, thus interrupts are enabled
                "sysretq",
                in(reg) self.stack.as_u64(),
                in("rcx") self.text.as_u64()
            );
        }
    }
}
