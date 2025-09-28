use core::arch::naked_asm;
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use alloc::collections::btree_map::BTreeMap;
use alloc::ffi::CString;
use alloc::vec;
use alloc::{sync::Arc, vec::Vec};
use conquer_once::spin::OnceCell;
use spin::RwLock;
use spin::mutex::Mutex;
use syscalls::syscall_ret;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameDeallocator, PageTable, PhysFrame};
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB},
};

use crate::PMM;
use crate::scheduler::Thread;
use crate::{debug_println, filesystem::vfs::Inode};
use elf::{LoadingError, ProgramHeaderEntry};

#[allow(dead_code)]
pub mod constants;

mod elf;
pub mod syscalls;

static NEXT_PID: AtomicU32 = AtomicU32::new(1);
static PROCESS_TABLE: OnceCell<ProcessTable> = OnceCell::uninit();

pub struct ProcessTable {
    /// Maps PID to user process
    processes: RwLock<BTreeMap<u32, Arc<Mutex<UserProcess>>>>,
}

impl ProcessTable {
    pub fn init() {
        PROCESS_TABLE.init_once(|| ProcessTable {
            processes: RwLock::new(BTreeMap::new()),
        });
    }

    /// Gets a process by its PID
    /// # Panics
    /// Panics if ProcessTable::init() has not been called.
    ///
    /// Most references to processes should be by PID. Holding this Arc<> for too long
    /// will delay process destruction, so drop this as soon as possible.
    pub fn get_by_pid(pid: u32) -> Option<Arc<Mutex<UserProcess>>> {
        PROCESS_TABLE
            .get()
            .expect("Expected ProcessTable::init() to have been called.")
            .processes
            .read()
            .get(&pid)
            .cloned()
    }

    /// Used internally when forking or creating a process to add to process table.
    /// # Panics
    /// Panics if ProcessTable::init() has not been called.
    fn add_process(process: UserProcess) {
        PROCESS_TABLE
            .get()
            .expect("Expected ProcessTable::init() to have been called.")
            .processes
            .write()
            .insert(process.pid, Arc::new(Mutex::new(process)));
    }
}

pub struct FileDescriptor {
    pub inode: Arc<Inode>,
    pub offset: u64,
    pub flags: u32,
}

pub struct UserProcess {
    /// Open file descriptors
    pub files: BTreeMap<u32, Arc<RwLock<FileDescriptor>>>, // So that file descriptors can be shared
    next_fd: u32, // TODO: be less naive (if you repeatedly open and close file descriptors you will run out)
    pub mapper: OffsetPageTable<'static>,
    pub thread: Arc<Mutex<Thread>>,
    pub pid: u32,
    /// Allocated frames
    frames: Vec<PhysFrame>,
    pub brk: VirtAddr,
    pub brk_initial: VirtAddr,
    pub cr3_frame: PhysFrame,
}

impl UserProcess {
    /// Used for creating the initial process.
    /// Reuses the initialisation page tables.
    /// Returns the PID of the new process.
    pub fn create(mapper: OffsetPageTable<'static>) -> u32 {
        let thread = Arc::new(Mutex::new(Thread::from_func(
            enter_userspace,
            None,
            None,
            None,
        )));

        let process = UserProcess {
            files: BTreeMap::new(),
            next_fd: 0,
            mapper,
            thread: thread.clone(),
            pid: NEXT_PID.fetch_add(1, Ordering::Relaxed),
            frames: vec![],
            brk: VirtAddr::new(0),
            brk_initial: VirtAddr::new(0),
            cr3_frame: Cr3::read().0,
        };

        thread.lock().process = Some(process.pid);
        thread.lock().cr3_frame = Some(process.cr3_frame);

        let pid = process.pid;

        ProcessTable::add_process(process);

        pid
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

        // Read program headers
        let headers: Vec<&ProgramHeaderEntry> = (0..header_num)
            .map(|i| header_start + header_size * i)
            .map(|offset| unsafe {
                &*(binary[offset..(offset + size_of::<ProgramHeaderEntry>())].as_ptr()
                    as *const ProgramHeaderEntry)
            })
            .collect();

        // Load program segments
        for header in &headers {
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
                //
                // EDIT: This is fine as long as page tables are cleaned up on process destruction (not implemented yet)
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

        // Set the program break to the end of the highest segment
        self.brk_initial = headers
            .iter()
            .map(|header| VirtAddr::new(header.virtual_address) + header.mem_size)
            .max()
            .unwrap_or(VirtAddr::new(0));
        self.brk = self.brk_initial;

        // https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/9388606854/artifacts/raw/x86-64-ABI/abi.pdf
        // See figure 3.9:
        // Note 7fff_ffff_0000..7fff_ffff_ffff forms the initial process stack
        let stack_top = VirtAddr::new(0x7fff_ffff_0000);
        let stack_len = 0x4000; // how much we are allocating for future growth of the stack

        // Alloc page for info
        unsafe {
            self.allocate_user_page(
                Page::<Size4KiB>::from_start_address(stack_top)
                    .expect("stack top to be page-aligned"),
                PageTableFlags::NO_EXECUTE,
            );
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
                self.allocate_user_page(page, PageTableFlags::NO_EXECUTE);
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

    /// Allocates a user accessible page to a new frame.
    pub unsafe fn allocate_user_page(&mut self, page: Page, flags: PageTableFlags) {
        let mut pmm = PMM.get().unwrap().lock();
        let frame = pmm.allocate_frame().expect("Could not allocate frame");

        unsafe {
            self.mapper
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
        self.frames.push(frame)
    }

    pub unsafe fn unmap_page(&mut self, page: Page) {
        let mut pmm = PMM.get().unwrap().lock();

        let (frame, flush) = self.mapper.unmap(page).unwrap();
        flush.flush();

        // We must remove the frame from the vectors to avoid a double free:
        // This is what makes the next deallocation sound.
        self.frames
            .swap_remove(self.frames.iter().position(|f| *f == frame).unwrap());

        unsafe {
            pmm.deallocate_frame(frame);
        }
    }

    fn fork_page_table(
        &self,
        src: &PageTable,
        lvl: usize,
    ) -> (&'static mut PageTable, Vec<PhysFrame>, PhysFrame) {
        let mut frames = vec![];

        let frame = PMM
            .get()
            .unwrap()
            .lock()
            .allocate_frame()
            .expect("no frame available");
        frames.push(frame);

        let dst: &mut PageTable = unsafe {
            &mut *(self.mapper.phys_offset() + frame.start_address().as_u64()).as_mut_ptr()
        };

        // Ensure destination page table is zeroed before writing to it
        unsafe { core::ptr::write_bytes(dst as *mut PageTable as *mut u8, 0, 4096) };

        // Trying to write *dst = PageTable::new() was problematic

        for (i, parent) in src.iter().enumerate() {
            if !parent.flags().contains(PageTableFlags::PRESENT) {
                continue;
            }
            if parent.flags().contains(PageTableFlags::USER_ACCESSIBLE) && (i < 256 || lvl < 4) {
                if lvl > 1 {
                    // Recurse
                    let (_, mut new_frames, frame) = self.fork_page_table(
                        unsafe { &*(self.mapper.phys_offset() + parent.addr().as_u64()).as_ptr() },
                        lvl - 1,
                    );
                    frames.append(&mut new_frames);

                    dst[i].set_frame(frame, parent.flags());
                } else {
                    // Copy raw page
                    let frame = PMM
                        .get()
                        .unwrap()
                        .lock()
                        .allocate_frame()
                        .expect("no frame available");

                    frames.push(frame);

                    let leaf_dst: &mut [u8] = unsafe {
                        slice::from_raw_parts_mut(
                            (self.mapper.phys_offset() + frame.start_address().as_u64())
                                .as_mut_ptr(),
                            frame.size() as usize,
                        )
                    };

                    leaf_dst.copy_from_slice(unsafe {
                        slice::from_raw_parts(
                            (self.mapper.phys_offset() + parent.addr().as_u64()).as_ptr(),
                            leaf_dst.len(),
                        )
                    });

                    dst[i].set_frame(frame, parent.flags());
                }
            } else {
                dst[i] = parent.clone(); // Only share kernel mappings
                debug_println!(
                    "cloning kernel mapping: {:?} lvl {} entry {}",
                    parent,
                    lvl,
                    i
                );
                // We can't share any other type of mapping or we'd double free.
            }
        }

        (dst, frames, frame)
    }

    /// Forks the process by creating a copy of all mappings
    /// and forking the thread. Returns the child PID.
    pub fn fork(&self) -> u32 {
        let (l4_table, frames, frame) = self.fork_page_table(self.mapper.level_4_table(), 4);
        let mapper = unsafe { OffsetPageTable::new(l4_table, self.mapper.phys_offset()) };

        let child = UserProcess {
            files: self.files.clone(),
            brk: self.brk,
            brk_initial: self.brk_initial,
            next_fd: self.next_fd,
            pid: NEXT_PID.fetch_add(1, Ordering::Relaxed),
            thread: Arc::new(Mutex::new(Thread::from_func(
                forked_entry,
                None,
                None,
                Some(frame),
            ))),
            frames,
            mapper,
            cr3_frame: frame,
        };

        child.thread.lock().process = Some(child.pid);

        let pid = child.pid;
        ProcessTable::add_process(child);

        pid
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

/// zeroes return and returns from syscall
#[unsafe(naked)]
unsafe extern "sysv64" fn forked_entry() {
    naked_asm!(
        "
        xor rax, rax // return 0
        jmp {}
        ", sym syscall_ret
    )
}
