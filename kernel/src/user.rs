use core::{
    arch::{asm, naked_asm},
    slice,
};

use alloc::vec;
use alloc::vec::Vec;
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{debug_println, kernel_log, memory::PhysicalMemoryManager, CPUS};

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
    kstack: Vec<u64>,    // Top of kernel stack
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
            kstack: vec![0; 2 * 4096],
        }
    }

    pub fn switch(&self) {
        unsafe {
            x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
                                                         // Switch kernel stack
            let top = VirtAddr::from_ptr(&self.kstack.last());
            let bottom = VirtAddr::from_ptr(&self.kstack);
            kernel_log!("top: {:?}, bottom: {:?}", top, bottom);
            CPUS.get().unwrap().get_cpu().set_kernel_stack(top);
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

extern "sysv64" fn get_kernel_stack() {
    CPUS.get().unwrap().get_cpu().get_kernel_stack();
}

extern "sysv64" fn handle_syscall_inner(
    syscall_number: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
) -> u64 {
    kernel_log!(
        "Syscall no: {} params: ({}, {}, {}, {})",
        syscall_number,
        arg0,
        arg1,
        arg2,
        arg3
    );
    42
}

#[naked]
pub unsafe extern "sysv64" fn handle_syscall() {
    // save registers required by sysretq
    naked_asm!(
        "
        // systretq uses these
        push rcx
        push r11

        push rbp // Will store old sp
        push rbx // Will store new sp

        push rax // sycall number
        push rdi // arg0
        push rsi // arg1
        push rdx // arg2
        push r10 // arg3

        call {} // Return value is now in rax
        mov rbx, rax // RBX = new sp

        // Restore syscall params
        pop r10
        pop rdx
        pop rsi
        pop rdi
        pop rax

        mov rbp, rsp // backup userspace stack
        mov rsp, rbx // switch to new stack

        // We push args to new stack
        push rax // sycall number
        push rdi // arg0
        push rsi // arg1
        push rdx // arg2
        push r10 // arg3

        // Pop to follow normal sysv64 calling convention
        pop r8
        pop rcx
        pop rdx
        pop rsi
        pop rdi

        call {}

        mov rsp, rbp // Restore userspace stack
        pop rbx
        pop rbp
        pop r11
        pop rcx
        sysretq",
        sym get_kernel_stack,
        sym handle_syscall_inner
    );
}
