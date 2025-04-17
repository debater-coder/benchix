use core::{
    arch::{asm, naked_asm},
    iter::zip,
    mem::transmute,
    ptr, slice,
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
    pub stack: VirtAddr, // Top of user stack
    kstack: Vec<u64>,    // Top of kernel stack
    rip: VirtAddr,
}

#[derive(Debug)]
pub enum LoadingError {
    InvalidHeader,
}

#[derive(Debug)]
#[repr(C)]
struct ProgramHeaderEntry {
    segment_type: u64, // contains both p_type and p_flags
    offset: u64,
    virtual_address: u64,
    unused: u64,
    image_size: u64,
    mem_size: u64,
    align: u64,
}

impl UserProcess {
    pub fn load_elf(
        mapper: &mut OffsetPageTable<'_>,
        pmm: &mut PhysicalMemoryManager,
        binary: &[u8],
    ) -> Result<Self, LoadingError> {
        // Validate ELF header
        if binary[0x0..0x4] != *b"\x7fELF" // Magic
            || binary[0x4] != 2 // 64-bit
            || binary[0x5] != 1 // Little endian
            || binary[0x10] != 2
        // Executable file
        {
            return Err(LoadingError::InvalidHeader);
        }
        let header_start = u64::from_ne_bytes(binary[0x20..0x28].try_into().unwrap()) as usize;
        let header_size = u16::from_ne_bytes(binary[0x36..0x38].try_into().unwrap()) as usize;
        let header_num = u16::from_ne_bytes(binary[0x38..0x3A].try_into().unwrap()) as usize;
        kernel_log!(
            "Headers: start: {:x} size: {:x} num: {}",
            header_start,
            header_size,
            header_num
        );

        if header_size < size_of::<ProgramHeaderEntry>() {
            return Err(LoadingError::InvalidHeader);
        }

        // Read program headers
        let headers: Vec<&ProgramHeaderEntry> = (0..header_num)
            .map(|i| header_start + header_size * i)
            .map(|offset| unsafe {
                &*(binary[offset..(offset + size_of::<ProgramHeaderEntry>())].as_ptr()
                    as *const ProgramHeaderEntry)
            })
            .collect();

        // Load program segments
        for header in headers {
            let segment_type = header.segment_type as u32;
            let segment_flags = (header.segment_type >> 32) as u32;

            if segment_type != 1 {
                // We only care about P_LOAD
                continue;
            }

            kernel_log!(
                "type: {:?} flags: {:?} other: {:x?}",
                segment_type,
                segment_flags,
                header
            );

            let exectuable = (segment_flags & 1) > 0;
            let writable = (segment_flags & 2) > 0;
            let readable = (segment_flags & 4) > 0;

            let contents =
                &binary[(header.offset as usize)..(header.offset + header.image_size) as usize];

            let num_frames = header.mem_size.div_ceil(0x1000);
            for i in 0..num_frames {
                let frame = pmm.allocate_frame().expect("Could not allocate frame.");

                let page = Page::<Size4KiB>::containing_address(VirtAddr::new(
                    header.virtual_address + i * 0x1000,
                ));

                // Since some pages will be read-only, we must write to the frame using the kernel's mappings, not the userspace mappings.
                let frame_offset = header.offset % 0x1000;
                let src = &contents
                    [(i as usize * 0x1000)..((i + 1) as usize * 0x1000).min(contents.len())];
                let dst: &mut [u8] = unsafe {
                    slice::from_raw_parts_mut(
                        (mapper.phys_offset() + frame.start_address().as_u64() + frame_offset)
                            .as_mut_ptr(),
                        src.len(),
                    )
                };

                dst.copy_from_slice(src);

                // Create mappings
                unsafe {
                    mapper
                        .map_to(
                            page,
                            frame,
                            PageTableFlags::PRESENT
                                | (if readable {
                                    PageTableFlags::USER_ACCESSIBLE
                                } else {
                                    PageTableFlags::empty()
                                })
                                | (if writable {
                                    PageTableFlags::WRITABLE
                                } else {
                                    PageTableFlags::empty()
                                })
                                | (if exectuable {
                                    PageTableFlags::empty()
                                } else {
                                    PageTableFlags::NO_EXECUTE
                                }),
                            pmm,
                        )
                        .expect("Failed to create mappings")
                        .flush();
                };
            }
        }
        kernel_log!("Mappings have been created.");

        let stack_end = VirtAddr::new(0x7fff_ffff_0000);
        let stack_content = [0u8; 4 * 0x1000];
        let stack_start = stack_end - stack_content.len() as u64 + 1;
        let stack_range = Page::range_inclusive(
            Page::<Size4KiB>::containing_address(stack_start),
            Page::<Size4KiB>::containing_address(stack_end),
        );
        unsafe {
            for page in stack_range {
                allocate_user_page(mapper, pmm, page, PageTableFlags::NO_EXECUTE);
            }
        }

        Ok(UserProcess {
            rip: VirtAddr::new(u64::from_ne_bytes(binary[0x18..0x20].try_into().unwrap())),
            kstack: vec![0; 2 * 4096],
            stack: stack_end,
        })
    }

    // pub fn new(
    //     mapper: &mut OffsetPageTable<'_>,
    //     pmm: &mut PhysicalMemoryManager,
    //     text_addr: VirtAddr,
    //     text_content: &[u8],
    //     stack_addr: VirtAddr,
    //     stack_content: &[u8],
    // ) -> Self {
    //     let text_range = Page::range_inclusive(
    //         Page::<Size4KiB>::containing_address(text_addr),
    //         Page::<Size4KiB>::containing_address(text_addr + (text_content.len() - 1) as u64),
    //     );

    //     let stack_end = stack_addr;
    //     let stack_start = stack_addr - stack_content.len() as u64 + 1;
    //     let stack_range = Page::range_inclusive(
    //         Page::<Size4KiB>::containing_address(stack_start),
    //         Page::<Size4KiB>::containing_address(stack_end),
    //     );
    //     unsafe {
    //         for page in text_range {
    //             allocate_user_page(mapper, pmm, page, PageTableFlags::empty());
    //         }

    //         for page in stack_range {
    //             allocate_user_page(mapper, pmm, page, PageTableFlags::NO_EXECUTE);
    //         }

    //         slice::from_raw_parts_mut(text_addr.as_mut_ptr::<u8>(), text_content.len())
    //             .copy_from_slice(text_content);
    //         slice::from_raw_parts_mut(stack_end.as_mut_ptr::<u8>(), stack_content.len())
    //             .copy_from_slice(stack_content);
    //     }

    //     UserProcess {
    //         text: text_addr,
    //         stack: stack_end,
    //         kstack: vec![0; 2 * 4096],
    //     }
    // }

    pub fn switch(&self) {
        kernel_log!("Sysret'ing to executable entry point: {:?}", self.rip);
        unsafe {
            x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
                                                         // Switch kernel stack
            let top = VirtAddr::from_ptr(&self.kstack.last());
            CPUS.get().unwrap().get_cpu().set_kernel_stack(top);
            asm!(
                "mov rsp, {}", // Stacks grow downwards
                "mov r11, 0x0202",             // Bit 9 is set, thus interrupts are enabled
                "sysretq",
                in(reg) self.stack.as_u64(),
                in("rcx") self.rip.as_u64()
            );
        }
    }
}

extern "sysv64" fn get_kernel_stack() -> u64 {
    CPUS.get().unwrap().get_cpu().get_kernel_stack().as_u64()
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
