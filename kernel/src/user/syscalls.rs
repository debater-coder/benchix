use core::{arch::naked_asm, ffi::CStr, slice};

use alloc::sync::Arc;
use spin::RwLock;
use x86_64::{
    registers::{
        model_specific::FsBase,
        segmentation::{Segment64, FS},
    },
    VirtAddr,
};

use crate::{
    filesystem::vfs::Filesystem,
    kernel_log,
    user::{
        constants::{EBADF, EFAULT, ENOSYS, O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY},
        FileDescriptor,
    },
    CPUS, VFS,
};

use super::{
    constants::{ARCH_SET_FS, EINVAL, ENOTTY},
    UserProcess,
};

extern "sysv64" fn get_kernel_stack() -> u64 {
    CPUS.get().unwrap().get_cpu().get_kernel_stack().as_u64()
}

/// Gets the current process (for syscalls)
/// # Panics
/// If there is no current process or the CPU struct isn't initialised
fn get_current_process() -> &'static mut UserProcess {
    CPUS.get()
        .unwrap()
        .get_cpu()
        .current_process
        .as_mut()
        .expect("No current process")
}

/// Returns true if an address is in userspace
/// Since this is a higher half kernel, userspace bits will be in the lower half.
fn check_addr(addr: VirtAddr) -> bool {
    addr.as_u64() & (1 << 63) == 0
}

/// Returns true if a buffer is in userspace
fn check_buffer(buffer: &[u8]) -> bool {
    let buffer_start = buffer.as_ptr();
    let buffer_end = unsafe { buffer_start.byte_add(buffer.len()) };

    check_addr(VirtAddr::from_ptr(buffer_start)) && check_addr(VirtAddr::from_ptr(buffer_end))
}

fn read(fd: u32, buf: *mut u8, count: usize) -> usize {
    kernel_log!("read({}, {:?}, {})", fd, buf, count);
    let buf = unsafe { slice::from_raw_parts_mut(buf, count) };
    if !check_buffer(buf) {
        return -EFAULT as usize;
    }

    let fd = get_current_process().files.get(&fd);

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
    kernel_log!("write({}, {:?}, {})", fd, buf, count);

    let buf = unsafe { slice::from_raw_parts_mut(buf, count) };
    if !check_buffer(buf) {
        return -EFAULT as usize;
    }

    let fd = get_current_process().files.get(&fd);

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
    kernel_log!("open({:?}, {:?})", pathname, flags);

    let process = get_current_process();

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

    kernel_log!("Opened to fd: {}", fd);
    fd as u64
}

fn close(fd: u32) -> u64 {
    kernel_log!("close({})", fd);
    0
}

fn exit(status: i32) -> ! {
    kernel_log!("Process exited with code {}", status);
    loop {}
}

fn arch_prctl(op: u32, addr: u64) -> u64 {
    kernel_log!("arch_prctl({:x}, {:x})", op, addr);
    match op {
        ARCH_SET_FS => {
            let addr = VirtAddr::new(addr);
            if !check_addr(addr) {
                return -EFAULT as u64;
            };

            FsBase::write(addr);
            0
        }
        _ => -EINVAL as u64,
    }
}

pub extern "sysv64" fn handle_syscall_inner(
    syscall_number: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
) -> u64 {
    match syscall_number {
        0 => read(arg0 as u32, arg1 as usize as *mut _, arg2 as usize) as u64,
        1 => write(arg0 as u32, arg1 as usize as *mut _, arg2 as usize) as u64,
        2 => open(arg0 as usize as *const _, arg1 as u32),
        3 => close(arg0 as u32),
        16 => -ENOTTY as u64, // ioctl
        158 => arch_prctl(arg0 as u32, arg1),
        231 => exit(arg0 as i32),
        60 => exit(arg0 as i32),
        _ => {
            kernel_log!(
                "Unknown syscall {}: ({}, {}, {}, {})",
                syscall_number,
                arg0,
                arg1,
                arg2,
                arg3
            );
            -ENOSYS as u64
        }
    }
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
