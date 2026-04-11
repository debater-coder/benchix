mod execve;

use core::{arch::naked_asm, ffi::CStr, mem::offset_of, slice};

use alloc::sync::Arc;
use execve::execve_inner;
use spin::{Mutex, RwLock};
use x86_64::{
    VirtAddr,
    registers::model_specific::FsBase,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageSize, PageTableFlags, Size4KiB,
        mapper::{MapToError, UnmapError},
    },
};

use crate::{
    PMM, VFS,
    cpu::PerCpu,
    filesystem::vfs::Filesystem,
    kernel_log,
    scheduler::{self, Thread, enqueue, yield_execution},
    user::{
        FileDescriptor, ProcessStatus,
        constants::{
            EBADF, EFAULT, ENOMEM, ENOSYS, MAP_ANONYMOUS, MAP_FIXED, O_ACCMODE, O_RDONLY, O_RDWR,
            O_WRONLY, PROT_EXEC, PROT_READ, PROT_WRITE,
        },
    },
};

use super::{
    ProcessTable, UserProcess,
    constants::{ARCH_SET_FS, ECHILD, EINVAL, ENOTTY, P_PID},
};

pub fn get_current_thread() -> Arc<Mutex<Thread>> {
    unsafe { PerCpu::get_cpu() }
        .current_thread
        .as_mut()
        .unwrap()
        .clone()
}

/// Gets the current process (for syscalls)
/// # Panics
/// If there is no current process or the CPU struct isn't initialised
fn get_current_process() -> Arc<Mutex<UserProcess>> {
    ProcessTable::get_by_pid(
        unsafe { PerCpu::get_cpu() }
            .current_thread
            .as_mut()
            .unwrap()
            .lock()
            .process
            .expect("No current process"),
    )
    .expect("No current process")
}

/// Returns true if an address is in userspace
/// Since this is a higher half kernel, userspace bits will be in the lower half.
fn check_addr(addr: VirtAddr) -> bool {
    !addr.is_null() && addr.as_u64() & (1 << 63) == 0
}

/// Returns true if a buffer is in userspace
fn check_buffer(buffer: &[u8]) -> bool {
    let buffer_start = buffer.as_ptr();
    let buffer_end = unsafe { buffer_start.byte_add(buffer.len()) };

    check_addr(VirtAddr::from_ptr(buffer_start))
        && check_addr(VirtAddr::from_ptr(buffer_end))
        && buffer_start <= buffer_end // To prevent buffers from wrapping around
}

fn checked_slice_from_raw_parts<'a, T>(data: *const T, len: usize) -> Option<&'a [T]> {
    let base = VirtAddr::from_ptr(data);
    let ptr_len = len.checked_mul(size_of::<T>());
    let end = ptr_len.and_then(|ptr_len| Some(base + (ptr_len as u64)));

    if check_addr(base)
        && (if let Some(end) = end {
            check_addr(end) && (base <= end)
        } else {
            false
        })
    {
        Some(unsafe { slice::from_raw_parts(data, len) })
    } else {
        None
    }
}

fn checked_slice_from_raw_parts_mut<'a, T>(data: *mut T, len: usize) -> Option<&'a mut [T]> {
    let base = VirtAddr::from_ptr(data);
    let ptr_len = len.checked_mul(size_of::<T>());
    let end = ptr_len.and_then(|len| Some(base + len as u64));

    if check_addr(base)
        && (if let Some(end) = end {
            check_addr(end) && base <= end
        } else {
            false
        })
    {
        Some(unsafe { slice::from_raw_parts_mut(data, len) })
    } else {
        None
    }
}

fn read(fd: u32, buf: *mut u8, count: usize) -> usize {
    debug_println!("read({}, {:?}, {})", fd, buf, count);
    let Some(buf) = checked_slice_from_raw_parts_mut(buf, count) else {
        return -EFAULT as usize;
    };

    let process = get_current_process();
    let process = process.lock();
    let fd = process.files.get(&fd);

    let mut fd = match fd {
        None => return -EBADF as usize,
        Some(fd) => fd.write(),
    };

    let access_mode = fd.flags & O_ACCMODE;

    if !(access_mode == O_RDWR || access_mode == O_RDONLY) {
        return -EBADF as usize;
    }

    let vfs = VFS.get().unwrap();

    let count = vfs.read(fd.inode.clone(), fd.offset, buf).unwrap();

    fd.offset += count as u64;

    count
}

fn write(fd: u32, buf: *mut u8, count: usize) -> usize {
    debug_println!("write({}, {:?}, {})", fd, buf, count);

    if count == 0 {
        return 0;
    }

    let Some(buf) = checked_slice_from_raw_parts(buf, count) else {
        return -EFAULT as usize;
    };

    debug_println!("waiting on process lock");
    let process = get_current_process();
    let process = process.lock();
    debug_println!("got process lock");
    let fd = process.files.get(&fd);

    let mut fd = match fd {
        None => return -EBADF as usize,
        Some(fd) => fd.write(),
    };

    let access_mode = fd.flags & O_ACCMODE;

    if !(access_mode == O_RDWR || access_mode == O_WRONLY) {
        return -EBADF as usize;
    }

    let vfs = VFS.get().unwrap();

    vfs.write(fd.inode.clone(), fd.offset, buf).unwrap();

    fd.offset += count as u64;

    count
}

fn open(pathname: *const i8, flags: u32) -> u64 {
    let pathname = unsafe { CStr::from_ptr(pathname) }.to_str().unwrap();
    assert!(check_buffer(pathname.as_bytes()));
    debug_println!("open({:?}, {:?})", pathname, flags);

    let process = get_current_process();
    let mut process = process.lock();

    let vfs = VFS.get().unwrap();

    let inode = vfs.traverse_fs(vfs.root.clone(), pathname).unwrap();

    vfs.open(inode.clone()).unwrap();

    let fd = process.next_fd;
    process.files.insert(
        fd,
        Arc::new(RwLock::new(FileDescriptor {
            inode,
            flags,
            offset: 0,
        })),
    );
    process.next_fd += 1;

    debug_println!("Opened to fd: {}", fd);
    fd as u64
}

fn close(fd: u32) -> u64 {
    debug_println!("close({})", fd);
    0
}

fn exit(status: i32) -> ! {
    debug_println!("exit({})", status);
    let process = get_current_process();

    process.lock().status = ProcessStatus::Terminated;

    // Release waiting threads
    for thread in process.lock().waiting.drain(..) {
        enqueue(thread);
    }

    kernel_log!("Process {} exited with code {}", process.lock().pid, status);
    loop {
        scheduler::yield_execution();
    }
}

fn arch_prctl(op: u32, addr: u64) -> u64 {
    debug_println!("arch_prctl({:x}, {:x})", op, addr);
    match op {
        ARCH_SET_FS => {
            let addr = VirtAddr::new(addr);
            if !check_addr(addr) {
                return -EFAULT as u64;
            };

            get_current_thread().lock().fs_base = addr;
            unsafe { FsBase::write(addr) };
            0
        }
        _ => -EINVAL as u64,
    }
}

fn execve(filename: *const i8, argv: *const *const i8, envp: *const *const i8) -> u64 {
    match execve_inner(filename, argv, envp) {
        Err(_) => u64::MAX,
    }
}

fn brk(addr: u64) -> u64 {
    debug_println!("brk({})", addr);

    let addr = VirtAddr::new(addr);
    let process = get_current_process();
    let mut process = process.lock();

    if !check_addr(addr) || addr < process.brk_initial || addr.is_null() {
        return process.brk.as_u64();
    }

    if addr > process.brk {
        for page in Page::range_inclusive(
            Page::<Size4KiB>::containing_address(process.brk),
            Page::containing_address(addr),
        )
        .skip(1)
        // First page has already been mapped so skip that one
        {
            if let Err(_) = unsafe {
                process.allocate_page(
                    page,
                    PageTableFlags::NO_EXECUTE
                        | PageTableFlags::USER_ACCESSIBLE
                        | PageTableFlags::WRITABLE,
                )
            } {
                return -ENOMEM as u64;
            };
        }
    }

    if addr < process.brk {
        for page in Page::range_inclusive(
            Page::<Size4KiB>::containing_address(addr),
            Page::containing_address(process.brk),
        )
        .skip(1)
        // Don't unmap the current break
        {
            unsafe {
                process.unmap_page(page).unwrap();
            }
        }
    }

    process.brk = addr;

    process.brk.as_u64()
}

fn fork() -> u32 {
    debug_println!("fork()");
    let child = get_current_process().lock().fork();

    let thread = ProcessTable::get_by_pid(child)
        .unwrap()
        .lock()
        .thread
        .clone();

    {
        let mut child_thread = thread.lock();

        let current_thread = get_current_thread();
        let current_thread = current_thread.lock();

        // Restore the "trapframe" (callee-saved registers saved at the top of kstack)
        let trapframe = current_thread.trapframe();
        child_thread.trapframe_mut().copy_from_slice(trapframe);

        // Set the correct FS_BASE
        child_thread.fs_base = current_thread.fs_base;

        // Thread::set_func() already sets the return address below the
        // trapframe, and the correct rsp so that on entering into the thread we
        // can pop from it.
    }

    enqueue(thread);

    child
}

fn waitid(id_type: u32, id: u32) -> u64 {
    debug_println!("waitid({}, {})", id_type, id);
    match id_type {
        P_PID => {
            if let Some(process) = ProcessTable::get_by_pid(id) {
                while process.lock().status != ProcessStatus::Terminated {
                    process.lock().waiting.push(get_current_thread());
                    yield_execution(); // yield without enqueue == wait
                }

                0
            } else {
                -ECHILD as u64
            }
        }
        _ => -EINVAL as u64,
    }
}

/// Stub implementation for set_tid_address
fn set_tid_address() -> u32 {
    debug_println!("set_tid_address()");
    get_current_process().lock().pid
}

/// ioctl stub
fn ioctl() -> u64 {
    debug_println!("ioctl()");
    -ENOTTY as u64
}

#[repr(C)]
struct Iovec {
    io_base: *mut u8,
    io_len: usize,
}

fn writev(fd: u32, iov: *mut Iovec, iovcnt: usize) -> usize {
    debug_println!("writev({}, 0x{:x}, {})", fd, iov as u64, iovcnt);

    let base = VirtAddr::from_ptr(iov);
    let len = (iovcnt as isize).checked_mul(size_of::<Iovec>() as isize);
    let end = len.and_then(|len| Some(VirtAddr::from_ptr(iov) + len as u64));

    if !(check_addr(base)
        && (if let Some(end) = end {
            check_addr(end) && base <= end
        } else {
            false
        }))
    {
        return -EFAULT as usize;
    }

    let iov = unsafe { slice::from_raw_parts(iov, iovcnt) };

    if iov
        .iter()
        .map(|entry| entry.io_len)
        .try_reduce(|x, y| x.checked_add(y))
        .is_none()
    {
        return -EINVAL as usize;
    }

    let mut bytes_written = 0;

    for io_entry in iov {
        let written = write(fd, io_entry.io_base, io_entry.io_len) as isize;
        if written >= 0 {
            bytes_written += written;
        } else {
            return written as usize;
        }
    }

    bytes_written as usize
}

pub const MMAP_BUMP_LIMIT: VirtAddr = VirtAddr::new(0x7e00_0000_0000);

fn mmap(addr: *mut u8, length: usize, prot: u32, flags: u32, _fd: u32, _offset: usize) -> u64 {
    // For now only support anonymous mappings
    if !(flags & MAP_ANONYMOUS > 0) {
        return -EINVAL as u64;
    }

    let fixed = flags & MAP_FIXED > 0; // Whether to place mapping exactly where specified
    let buf = checked_slice_from_raw_parts_mut(addr, length);

    let addr = VirtAddr::from_ptr(addr);

    let range = if fixed {
        // Create the mapping exactly where requested
        let start = Page::<Size4KiB>::from_start_address(addr);
        let end = Page::from_start_address(addr + length as u64);
        if let (Ok(start), Ok(end), Some(_)) = (start, end, buf) {
            Some(Page::range(start, end))
        } else {
            None
        }
    } else {
        // Choose a mapping as a bump allocation
        let process = get_current_process();
        let mut process = process.lock();
        let length = length as u64;

        let num_pages = length.div_ceil(Size4KiB::SIZE);
        let curr_base = process.mmap_base;

        process.mmap_base -= Size4KiB::SIZE * num_pages;

        if process.mmap_base < MMAP_BUMP_LIMIT {
            return -ENOMEM as u64;
        }

        Some(Page::range(
            Page::from_start_address(process.mmap_base).unwrap(),
            Page::from_start_address(curr_base).unwrap(),
        ))
    };

    if let Some(range) = range {
        let process = get_current_process();
        let mut process = process.lock();

        for page in range.clone() {
            let mut flags = PageTableFlags::PRESENT;
            flags.set(PageTableFlags::USER_ACCESSIBLE, prot & PROT_READ > 0);
            flags.set(PageTableFlags::WRITABLE, prot & PROT_WRITE > 0);
            flags.set(PageTableFlags::NO_EXECUTE, prot & PROT_EXEC == 0);

            let map_result = unsafe { process.allocate_page(page, flags) };

            match map_result {
                Ok(_) => {}
                Err(MapToError::FrameAllocationFailed) => return -ENOMEM as u64,
                Err(MapToError::ParentEntryHugePage) => panic!("Huge pages not implemented"),
                Err(MapToError::PageAlreadyMapped(_)) => {
                    let _ = process.mapper.unmap(page);

                    match unsafe { process.allocate_page(page, flags) } {
                        Ok(_) => {}
                        Err(MapToError::FrameAllocationFailed) => return -ENOMEM as u64,
                        Err(MapToError::ParentEntryHugePage) => {
                            panic!("Huge pages not implemented")
                        }
                        Err(MapToError::PageAlreadyMapped(frame)) => {
                            panic!("Failed to remove mapping {:?}", frame);
                        }
                    }
                }
            }
        }

        range.start.start_address().as_u64()
    } else {
        -EINVAL as u64
    }
}

pub extern "sysv64" fn handle_syscall_inner(syscall_number: u64) -> u64 {
    let (arg0, arg1, arg2, arg3, arg4, arg5) = {
        let current_thread = get_current_thread();
        let current_thread = current_thread.lock();
        let trapframe = current_thread.trapframe();
        (
            trapframe[0],
            trapframe[1],
            trapframe[2],
            trapframe[3],
            trapframe[4],
            trapframe[5],
        )
    };

    let retval = match syscall_number {
        0 => read(arg0 as u32, arg1 as usize as *mut _, arg2 as usize) as u64,
        1 => write(arg0 as u32, arg1 as usize as *mut _, arg2 as usize) as u64,
        2 => open(arg0 as usize as *const _, arg1 as u32),
        3 => close(arg0 as u32),
        9 => mmap(
            arg0 as *mut u8,
            arg1 as usize,
            arg2 as u32,
            arg3 as u32,
            arg4 as u32,
            arg5 as usize,
        ) as u64,
        12 => brk(arg0),
        16 => ioctl(),
        20 => writev(arg0 as u32, arg1 as usize as *mut _, arg2 as usize) as u64,
        57 => fork() as u64,
        59 => execve(
            arg0 as usize as *const _,
            arg1 as usize as *const _,
            arg2 as usize as *const _,
        ),
        60 => exit(arg0 as i32),
        61 => waitid(P_PID, arg0 as u32), // stub for wait4
        158 => arch_prctl(arg0 as u32, arg1),
        218 => set_tid_address() as u64,
        231 => exit(arg0 as i32), // exit_group
        247 => waitid(arg0 as u32, arg1 as u32),
        _ => {
            debug_println!(
                "Unknown syscall {}: ({}, {}, {}, {}, {}, {})",
                syscall_number,
                arg0,
                arg1,
                arg2,
                arg3,
                arg4,
                arg5
            );
            -ENOSYS as u64
        }
    };
    debug_println!("returned {:?}", retval);
    retval
}

#[unsafe(naked)]
pub unsafe extern "sysv64" fn handle_syscall() {
    // save registers required by sysretq
    naked_asm!(
        "
        mov gs:[{ustack_off}], rsp    // save the userspace stack into ustack
        mov rsp, gs:[{kstack_off}]    // load the kernel stack

        // === FROM NOW ON WE ARE ON KERNEL STACK ===

        /// AT THIS POINT THE KERNEL STACK SHOULD BE EMPTY (the following should be pushed at the base)

        // Create a trapframe at the base of the stack
        // This is used to return to userspace
        push gs:[{ustack_off}] // trapframe[14]
        push rbx
        push rcx
        push rbp
        push r11
        push r12
        push r13
        push r14
        push r15
        push r9
        push r8
        push r10
        push rdx
        push rsi
        push rdi // trapframe[0]

        mov rdi, rax // pass rax as first param

        call {handle_syscall_inner}

        jmp {syscall_ret}
        ",
        ustack_off = const(offset_of!(PerCpu, ustack)),
        kstack_off = const(offset_of!(PerCpu, kstack)),
        handle_syscall_inner = sym handle_syscall_inner,
        syscall_ret = sym syscall_ret
    );
}

/// Handles returning to userspace using a trapframe stored at base of stack
#[unsafe(naked)]
pub unsafe extern "sysv64" fn syscall_ret() {
    naked_asm!(
        "
        pop rdi
        pop rsi
        pop rdx
        pop r10
        pop r8
        pop r9
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop rbp
        pop rcx
        pop rbx

        pop rsp
        sysretq
        "
    )
}
