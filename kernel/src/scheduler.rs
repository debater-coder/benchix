use core::{
    mem::offset_of,
    sync::atomic::{AtomicU32, Ordering},
};

use alloc::{
    borrow::ToOwned,
    collections::vec_deque::VecDeque,
    string::String,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use conquer_once::spin::OnceCell;
use spin::Mutex;
use x86_64::{
    instructions::{
        hlt,
        interrupts::{self, enable, enable_and_hlt},
    },
    VirtAddr,
};

use crate::{kernel_log, user::UserProcess, CPUS};

static READY: OnceCell<Mutex<VecDeque<Arc<Mutex<Thread>>>>> = OnceCell::uninit();
static NEXT_TID: AtomicU32 = AtomicU32::new(0);

/// Used Redox for reference.
/// https://gitlab.redox-os.org/redox-os/kernel/-/blob/master/src/context/arch/x86_64.rs?ref_type=heads
///
/// These are all System V ABI callee-saved registers, the rest will be pushed
/// to stack on function call
#[derive(Default, Clone, Debug)]
#[repr(C)]
pub struct Context {
    pub rflags: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rbp: u64,
    pub rsp: u64,
}

impl Context {
    /// Creates a blank context, values will be saved on switch
    pub fn new() -> Self {
        Context::default()
    }
}

pub struct Thread {
    pub context: Context,
    /// Kernel stack
    pub kstack: Vec<u64>,
    /// Parent process
    pub process: Weak<Mutex<UserProcess>>,
    /// Thread id
    pub tid: u32,
    pub name: Option<String>,
}

impl core::fmt::Debug for Thread {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Thread")
            .field("name", &self.name.clone().unwrap_or("<no name>".to_owned()))
            .field("context", &format_args!("{:x?}", self.context))
            .field("process", &format_args!("{:?}", self.process))
            .finish()
    }
}

impl Thread {
    pub fn from_func(
        func: unsafe extern "sysv64" fn(),
        process: Weak<Mutex<UserProcess>>,
        name: Option<String>,
    ) -> Thread {
        let mut thread = Thread {
            context: Context::new(),
            kstack: vec![0; 2 * 4096],
            process,
            tid: NEXT_TID.fetch_add(1, Ordering::Relaxed),
            name,
        };

        // Put the return address on the top of the stack
        *thread.kstack.last_mut().unwrap() = func as u64;

        thread.context.rsp = thread.kstack.last_mut().unwrap() as *const u64 as u64;
        thread
    }
}

pub fn init() {
    READY
        .try_init_once(|| Mutex::new(VecDeque::new()))
        .expect("scheduler::init should only be called once.")
}

pub fn enqueue(thread: Arc<Mutex<Thread>>) {
    READY
        .get()
        .expect("scheduler::init should have been called")
        .lock()
        .push_back(thread);
}

/// Taken from redox os, with some modifications
#[naked]
unsafe extern "sysv64" fn switch_to(_prev: &mut Context, _next: &Context) {
    // prev = rdi, next = rsi
    // The next context is a read-only clone, to save us from having to deal with its lock
    core::arch::naked_asm!(
        concat!("
            // Save old registers, and load new ones
            mov [rdi + {off_rbx}], rbx
            mov rbx, [rsi + {off_rbx}]

            mov [rdi + {off_r12}], r12
            mov r12, [rsi + {off_r12}]

            mov [rdi + {off_r13}], r13
            mov r13, [rsi + {off_r13}]

            mov [rdi + {off_r14}], r14
            mov r14, [rsi + {off_r14}]

            mov [rdi + {off_r15}], r15
            mov r15, [rsi + {off_r15}]

            mov [rdi + {off_rbp}], rbp
            mov rbp, [rsi + {off_rbp}]

            mov [rdi + {off_rsp}], rsp
            mov rsp, [rsi + {off_rsp}]

            // push RFLAGS (can only be modified via stack)
            pushfq
            // pop RFLAGS into `self.rflags`
            pop QWORD PTR [rdi + {off_rflags}]

            // push `next.rflags`
            push QWORD PTR [rsi + {off_rflags}]
            // pop into RFLAGS
            popfq

            // When we return, we cannot even guarantee that the return address on the stack, points to
            // the calling function, `context::switch`. Thus, we have to execute this Rust hook by
            // ourselves, which will unlock the contexts before the later switch.

            // Note that switch_finish_hook will be responsible for executing `ret`.
            jmp {switch_hook}
            "),

        off_rflags = const(offset_of!(Context, rflags)),

        off_rbx = const(offset_of!(Context, rbx)),
        off_r12 = const(offset_of!(Context, r12)),
        off_r13 = const(offset_of!(Context, r13)),
        off_r14 = const(offset_of!(Context, r14)),
        off_r15 = const(offset_of!(Context, r15)),
        off_rbp = const(offset_of!(Context, rbp)),
        off_rsp = const(offset_of!(Context, rsp)),

        switch_hook = sym switch_finish_hook,
    );
}

/// Releases locks and sets current thread
unsafe extern "sysv64" fn switch_finish_hook() {
    let cpu = CPUS.get().unwrap().get_cpu();
    if let Some(thread) = cpu.current_thread.as_mut() {
        thread.force_unlock();
    }

    cpu.current_thread = cpu.next_thread.clone();
    cpu.next_thread = None;

    cpu.set_ist(VirtAddr::new(
        cpu.current_thread.clone().unwrap().lock().context.rsp,
    ));
}

/// Yields to scheduler, but keep current thread in queue.
pub fn yield_and_continue() {
    if let Some(thread) = CPUS.get().unwrap().get_cpu().current_thread.as_ref() {
        enqueue(thread.clone());
    }
    yield_execution();
}

/// Yields to scheduler to decide what should use CPU time.
pub fn yield_execution() {
    interrupts::disable();

    let cpu = CPUS.get().unwrap().get_cpu();
    let next_thread = {
        READY
            .get()
            .expect("scheduler::init should have been called")
            .lock()
            .pop_front()
    }
    .unwrap_or(cpu.idle_thread.clone());

    let current_thread = cpu.current_thread.as_mut();

    let prev: &mut Context = match current_thread {
        None => &mut Context::new(), // Dummy context
        Some(thread) => {
            // If the next thread and the current thread is the same, we will deadlock
            if Arc::ptr_eq(&thread.clone(), &next_thread) {
                debug_print!(".");
                return;
            }
            debug_println!("Switching from {:?} to {:?}", thread, next_thread);
            &mut thread.lock().context
        }
    };

    let next = { next_thread.lock().context.clone() }; // The lock will be released after this

    CPUS.get().unwrap().get_cpu().next_thread = Some(next_thread.clone());

    unsafe {
        switch_to(prev, &next);
    }
}
