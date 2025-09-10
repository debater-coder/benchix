use crate::{
    CPUS, filesystem,
    scheduler::{enqueue, yield_execution},
    user::{
        enter_userspace,
        syscalls::{check_addr, check_buffer, get_current_process},
    },
};
use alloc::{borrow::ToOwned, vec};
use core::ffi::CStr;
use x86_64::VirtAddr;

pub(super) struct ExecveError;

pub(super) fn execve_inner(
    filename: *const i8,
    argv: *const *const i8,
    _envp: *const *const i8,
) -> Result<!, ExecveError> {
    debug_println!("execve");
    if filename.is_null() {
        return Err(ExecveError);
    }
    let filename = unsafe { CStr::from_ptr(filename) }.to_str().unwrap(); // TODO check filename not null
    if !check_buffer(filename.as_bytes()) {
        return Err(ExecveError);
    }

    let mut args = vec![];

    if !argv.is_null() {
        // max of 256 args to avoid DoSing the kernel
        for i in 0..256 {
            let curr_argv_ptr = unsafe { argv.add(i) };
            if !check_addr(VirtAddr::from_ptr(curr_argv_ptr)) {
                return Err(ExecveError);
            }

            if unsafe { *curr_argv_ptr }.is_null() {
                break;
            }

            let arg = unsafe { CStr::from_ptr(*curr_argv_ptr) }.to_str().unwrap();
            if !check_buffer(arg.as_bytes()) {
                return Err(ExecveError);
            }

            // By casting to an owned String on the kernel heap, it will survive to after the page table is cleared
            args.push(arg.to_owned());
        }
    }

    debug_println!("execve({:?}, {:?})", filename, args);

    let process = get_current_process();

    let binary = filesystem::read(filename).map_err(|_| ExecveError)?;

    let execve_result = process.lock().execve(
        binary.as_slice(),
        args.iter().map(|s| &**s).collect(),
        vec![],
    );
    match execve_result {
        Ok(_) => {
            {
                let process = process.lock(); // In a block to ensure mutex guard is dropped before scheduler

                // Prevent context switch from saving current state (and overriding execve's work)
                CPUS.get().unwrap().get_cpu().current_thread = None;

                // Set entry point of process to switch to the userspace entry point (bypassing normal syscall machinery)
                process.thread.lock().set_func(enter_userspace);

                // We need to requeue the thread manually since yield_and_continue() relies on requeuing the current thread
                enqueue(process.thread.clone());
            }

            yield_execution();

            panic!("Re-entered invalid thread: execve syscall");
        }
        Err(_) => Err(ExecveError),
    }
}
