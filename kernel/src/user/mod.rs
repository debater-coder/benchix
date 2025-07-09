use core::ffi::CStr;
use core::ptr::slice_from_raw_parts_mut;
use core::{arch::asm, slice};

use alloc::ffi::CString;
use alloc::string::String;
use alloc::{collections::btree_map::BTreeMap, vec};
use alloc::{sync::Arc, vec::Vec};
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{
    debug_println, filesystem::vfs::Inode, kernel_log, memory::PhysicalMemoryManager, CPUS,
};

pub mod syscalls;

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
    pub files: BTreeMap<u32, Arc<Inode>>,
    next_fd: u32,
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
        args: Vec<&str>,
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

            let page_range = Page::range_inclusive(
                Page::<Size4KiB>::containing_address(VirtAddr::new(header.virtual_address)),
                Page::containing_address(VirtAddr::new(header.virtual_address + header.mem_size)),
            );

            for page in page_range {
                let frame = pmm.allocate_frame().expect("Could not allocate frame.");

                let start_index = page
                    .start_address()
                    .as_u64()
                    .saturating_sub(VirtAddr::from_ptr(contents.as_ptr()).as_u64())
                    as usize;
                let src = &contents[start_index..(start_index + 0x1000).min(contents.len())];

                let frame_offset = VirtAddr::from_ptr(src.as_ptr()).as_u64() % 0x1000;

                let dst = unsafe {
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

        let mut stack_end = VirtAddr::new(0x7fff_ffff_0000);
        let stack_len = 0x4000;

        let stack_range = Page::range_inclusive(
            Page::<Size4KiB>::containing_address(stack_end - stack_len),
            Page::<Size4KiB>::containing_address(stack_end),
        );
        unsafe {
            for page in stack_range {
                allocate_user_page(mapper, pmm, page, PageTableFlags::NO_EXECUTE);
            }
        }

        let mut argv = vec![];
        let len = args.len() as u64;

        for arg in args.iter().rev() {
            // Push string onto stack
            let src = CString::new(*arg).unwrap();
            let src = src.as_bytes_with_nul();
            stack_end -= src.len() as u64;
            let dest: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(stack_end.as_mut_ptr(), src.len()) };
            dest.copy_from_slice(src);

            // Store pointer in vector
            argv.push(stack_end.as_u64());
        }

        // Push argv
        for arg in argv {
            stack_end -= size_of::<u64>() as u64;
            let dest: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(stack_end.as_mut_ptr(), size_of::<u64>()) };
            dest.copy_from_slice(arg.to_ne_bytes().as_slice());
        }

        // Push argc
        stack_end -= size_of::<u64>() as u64;
        let dest: &mut [u8] =
            unsafe { slice::from_raw_parts_mut(stack_end.as_mut_ptr(), size_of::<u64>()) };
        dest.copy_from_slice(len.to_ne_bytes().as_slice());

        Ok(UserProcess {
            rip: VirtAddr::new(u64::from_ne_bytes(binary[0x18..0x20].try_into().unwrap())),
            kstack: vec![0; 2 * 4096],
            stack: stack_end,
            files: BTreeMap::new(),
            next_fd: 0,
        })
    }

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
