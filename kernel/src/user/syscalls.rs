use bitflags::bitflags;
use core::{arch::naked_asm, ffi::CStr, ptr::slice_from_raw_parts_mut, slice};

use x86_64::VirtAddr;

use crate::{filesystem::vfs::Filesystem, kernel_log, CPUS, VFS};

extern "sysv64" fn get_kernel_stack() -> u64 {
    CPUS.get().unwrap().get_cpu().get_kernel_stack().as_u64()
}

/// Checks if an address is in userspace
/// Since this is a higher half kernel, userspace bits will be in the lower half.
fn check_addr(addr: VirtAddr) -> bool {
    addr.as_u64() & (1 << 63) == 0
}

fn check_buffer(buffer: &[u8]) -> bool {
    let buffer_start = buffer.as_ptr();
    let buffer_end = unsafe { buffer_start.byte_add(buffer.len()) };

    check_addr(VirtAddr::from_ptr(buffer_start)) && check_addr(VirtAddr::from_ptr(buffer_end))
}

fn read(fd: u32, buf: *mut u8, count: usize) -> usize {
    kernel_log!("read({}, {:?}, {})", fd, buf, count);
    let buf = unsafe { slice::from_raw_parts_mut(buf, count) };
    assert!(check_buffer(buf));

    let inode = CPUS
        .get()
        .unwrap()
        .get_cpu()
        .current_process
        .as_ref()
        .unwrap()
        .files
        .get(&fd)
        .unwrap()
        .clone();

    kernel_log!("reading to inode: {:?} with fd {}", inode, fd);

    let vfs = VFS.get().unwrap();

    vfs.read(inode, 0, buf).unwrap()
}

fn write(fd: u32, buf: *mut u8, count: usize) -> usize {
    kernel_log!("write({}, {:?}, {})", fd, buf, count);

    let buf = unsafe { slice::from_raw_parts_mut(buf, count) };
    assert!(check_buffer(buf));

    let inode = CPUS
        .get()
        .unwrap()
        .get_cpu()
        .current_process
        .as_ref()
        .unwrap()
        .files
        .get(&fd)
        .unwrap()
        .clone();

    let vfs = VFS.get().unwrap();

    vfs.write(inode, 0, buf).unwrap();

    count
}

fn open(pathname: *const i8, flags: u32) -> u64 {
    let pathname = unsafe { CStr::from_ptr(pathname) }.to_str().unwrap();
    assert!(check_buffer(pathname.as_bytes()));
    kernel_log!("open({:?}, {:?})", pathname, flags);

    // TODO: care about the flags

    let process = CPUS
        .get()
        .unwrap()
        .get_cpu()
        .current_process
        .as_mut()
        .unwrap();

    let vfs = VFS.get().unwrap();

    let inode = vfs.traverse_fs(vfs.root.clone(), pathname).unwrap();

    vfs.open(inode.clone()).unwrap();

    let fd = process.next_fd;
    process.files.insert(fd, inode);
    process.next_fd += 1;

    kernel_log!("Opened to fd: {}", fd);
    fd as u64
}

fn close(fd: u32) -> u32 {
    kernel_log!("close({}", fd);
    0
}

fn exit(status: i32) -> ! {
    kernel_log!("Process exited with code {}", status);
    loop {}
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
        3 => {
            kernel_log!("close");
            0
        }
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
            0
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
