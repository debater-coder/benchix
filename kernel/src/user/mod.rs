use core::arch::naked_asm;
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use alloc::collections::btree_map::BTreeMap;
use alloc::ffi::CString;
use alloc::sync::Weak;
use alloc::vec;
use alloc::{sync::Arc, vec::Vec};
use spin::{Mutex, RwLock};
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{FrameDeallocator, PhysFrame};
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
};

use crate::PMM;
use crate::scheduler::Thread;
use crate::{debug_println, filesystem::vfs::Inode};

#[allow(dead_code)]
pub mod constants;

pub mod syscalls;

static NEXT_PID: AtomicU32 = AtomicU32::new(1);

unsafe fn allocate_user_page(
    mapper: &mut OffsetPageTable,
    page: Page,
    flags: PageTableFlags,
) -> PhysFrame {
    let mut pmm = PMM.get().unwrap().lock();
    let frame = pmm.allocate_frame().expect("Could not allocate frame");

    unsafe {
        mapper
            .map_to(
                page,
                frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE
                    | flags,
                &mut *pmm,
            )
            .unwrap()
            .flush()
    };
    frame
}

pub struct UserProcess {
    /// Open file descriptors
    pub files: BTreeMap<u32, Arc<RwLock<FileDescriptor>>>, // So that file descriptors can be shared
    next_fd: u32, // TODO: be less naive (if you repeatedly open and close file descriptors you will run out)
    #[allow(dead_code)]
    cr3: (PhysFrame, Cr3Flags),
    pub mapper: OffsetPageTable<'static>,
    pub thread: Arc<Mutex<Thread>>,
    pub pid: u32,
    /// Allocated frames
    frames: Vec<PhysFrame>,
}

pub struct FileDescriptor {
    pub inode: Arc<Inode>,
    pub offset: u64,
    pub flags: u32,
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
    /// Used for creating the initial process.
    /// Reuses the initialisation page tables
    pub fn new(mapper: OffsetPageTable<'static>) -> Arc<Mutex<Self>> {
        let thread = Arc::new(Mutex::new(Thread::from_func(
            enter_userspace,
            Weak::new(),
            None,
        )));

        let process = Arc::new(Mutex::new(UserProcess {
            files: BTreeMap::new(),
            next_fd: 0,
            cr3: Cr3::read(),
            mapper,
            thread: thread.clone(),
            pid: NEXT_PID.fetch_add(1, Ordering::Relaxed),
            frames: vec![],
        }));

        thread.lock().process = Arc::downgrade(&process);

        process
    }

    /// See the POSIX execve system call for information on how it is used
    /// Currently this only supports static ELF loading -- dynamic executables or
    /// shebang scripts are not supported.
    ///
    pub fn execve(
        &mut self,
        binary: &[u8],
        args: Vec<&str>,
        _env: Vec<&str>, // TODO
    ) -> Result<(), LoadingError> {
        // Validate ELF header
        if binary[0x0..0x4] != *b"\x7fELF" // Magic
            || binary[0x4] != 2 // 64-bit
            || binary[0x5] != 1 // Little endian
            || binary[0x10] != 2
        // Executable file
        {
            debug_println!("{:?}", &binary[0x0..=0x10]);
            return Err(LoadingError::InvalidHeader);
        }
        let header_start = u64::from_ne_bytes(binary[0x20..0x28].try_into().unwrap()) as usize;
        let header_size = u16::from_ne_bytes(binary[0x36..0x38].try_into().unwrap()) as usize;
        let header_num = u16::from_ne_bytes(binary[0x38..0x3A].try_into().unwrap()) as usize;
        debug_println!(
            "Headers: start: {:x} size: {:x} num: {}",
            header_start,
            header_size,
            header_num
        );

        if header_size < size_of::<ProgramHeaderEntry>() {
            return Err(LoadingError::InvalidHeader);
        }

        // Clear previous userspace mappings (the entire lower half of the kernel)
        for entry in self.mapper.level_4_table_mut().iter_mut().take(256) {
            entry.set_unused();
        }

        // Dealloc previous frames
        for frame in self.frames.drain(..) {
            unsafe {
                PMM.get().unwrap().lock().deallocate_frame(frame);
            }
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

            debug_println!(
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
                let frame = PMM
                    .get()
                    .unwrap()
                    .lock()
                    .allocate_frame()
                    .expect("Could not allocate frame.");

                let start_index = page
                    .start_address()
                    .as_u64()
                    .saturating_sub(VirtAddr::from_ptr(contents.as_ptr()).as_u64())
                    as usize;
                let src = &contents[start_index..(start_index + 0x1000).min(contents.len())];

                let dst = unsafe {
                    slice::from_raw_parts_mut(
                        (self.mapper.phys_offset() + frame.start_address().as_u64()).as_mut_ptr(),
                        src.len(),
                    )
                };

                dst.copy_from_slice(src);

                debug_println!("mapping {:?} to {:?}, len: {:?}", page, frame, src.len());

                // Create mappings
                // This looks like it leaks memory since map_to() can map frames when creating page tables.
                // However there will only ever be a finite amount of page tables, so this is fine.
                unsafe {
                    self.mapper
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
                            &mut *PMM.get().unwrap().lock(),
                        )
                        .expect("Failed to create mappings")
                        .flush();
                };

                self.frames.push(frame);
            }
        }
        debug_println!("Mappings have been created.");

        // https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/9388606854/artifacts/raw/x86-64-ABI/abi.pdf
        // See figure 3.9:
        // Note 7fff_ffff_0000..7fff_ffff_ffff forms the initial process stack
        let stack_top = VirtAddr::new(0x7fff_ffff_0000);
        let stack_len = 0x4000; // how much we are allocating for future growth of the stack

        // Alloc page for info
        unsafe {
            self.frames.push(allocate_user_page(
                &mut self.mapper,
                Page::<Size4KiB>::from_start_address(stack_top)
                    .expect("stack top to be page-aligned"),
                PageTableFlags::NO_EXECUTE,
            ));
        }

        let argc = args.len() as u64;

        // argc
        unsafe {
            stack_top.as_mut_ptr::<u64>().write(argc);
        }

        // argv and argv strings
        let mut argv_base = stack_top + 8 + 8 * argc + 8 + 8 + 8; // Where the actual strings will be stored

        for (i, arg) in args.iter().enumerate() {
            // Pointer to argv string
            unsafe {
                (stack_top + 8 + 8 * i as u64)
                    .as_mut_ptr::<u64>()
                    .write(argv_base.as_u64());
            }

            // Actual argv string (null terminated)
            let src = CString::new(*arg).unwrap();
            let src = src.as_bytes_with_nul();

            let dest: &mut [u8] =
                unsafe { slice::from_raw_parts_mut(argv_base.as_mut_ptr(), src.len()) };

            dest.copy_from_slice(src);
            argv_base += src.len() as u64;
        }

        // so that argv[argc] = 0
        // Technically this should be zeroed already but we do it so I don't forget to leave a gap
        unsafe {
            (stack_top + 8 + 8 * argc).as_mut_ptr::<u64>().write(0);
        }

        // No environment variables yet so we just terminate the envp array with another 0
        unsafe {
            (stack_top + 8 + 8 * argc + 8).as_mut_ptr::<u64>().write(0);
        }

        // No aux variables so yet another 0u64
        unsafe {
            (stack_top + 8 + 8 * argc + 8 + 8)
                .as_mut_ptr::<u64>()
                .write(0);
        }

        // Allocate the rest of the stack
        let stack_range = Page::range(
            Page::<Size4KiB>::containing_address(stack_top - stack_len), // Future top of stack
            Page::<Size4KiB>::containing_address(stack_top),             // Current top of stack
        );

        unsafe {
            for page in stack_range {
                self.frames.push(allocate_user_page(
                    &mut self.mapper,
                    page,
                    PageTableFlags::NO_EXECUTE,
                ));
            }
        }

        // Userspace entry point
        let entry = u64::from_ne_bytes(binary[0x18..0x20].try_into().unwrap());
        self.thread.lock().context.rbp = entry;

        // Userspace stack pointer
        self.thread.lock().context.rbx = stack_top.as_u64();

        debug_println!("Userspace entry point {:x}", entry);

        Ok(())
    }
}

/// Enters userspace, enabling interrupts. Since thread entry points
/// can't take parameters:
/// - rbp stores userspace entry point
/// - rbx stores userspace stack pointer
#[unsafe(naked)]
unsafe extern "sysv64" fn enter_userspace() {
    naked_asm!(
        // We must keep the userspace stack in rbx, since the kstack
        // is used to 'return' into here.
        "mov rsp, rbx
        mov rcx, rbp
        mov r11, 0x0202
        sysretq"
    )
}
