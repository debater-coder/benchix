#![feature(abi_x86_interrupt, new_zeroed_alloc)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use conquer_once::spin::OnceCell;
use core::arch::asm;
use cpu::PerCpu;
use filesystem::devfs::Devfs;
use filesystem::initrd::Initrd;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use lapic::Lapic;
use memory::PhysicalMemoryManager;
use user::UserProcess;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags};
use x86_64::VirtAddr;

mod console;
mod cpu;
mod filesystem;
mod interrupts;
mod lapic;
mod memory;
mod panic;
mod user;

use crate::console::Console;
use alloc::{format, vec};
use bootloader_api::config::Mapping;
use bootloader_api::BootloaderConfig;
use x86_64::instructions::hlt;

pub const HEAP_START: u64 = 0x_ffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_f700_0000_0000;
pub const KERNEL_STACK_SIZE: u64 = 80 * 1024; // 80 Kb
pub const LAPIC_START_VIRT: u64 = 0xffff_8fff_ffff_0000;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.kernel_stack_size = KERNEL_STACK_SIZE;
    config.mappings.kernel_stack = Mapping::FixedAddress(KERNEL_STACK_START);
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0xffff_e000_0000_0000)); // 16 TiB of RAM ought to be enough for anybody
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);
    config.mappings.dynamic_range_end = Some(0xffff_8fff_fffe_ffff);
    config
};

macro_rules! boot_log {
    ($console:expr, $($arg:tt)*) => {
            debug_println!("boot: {}", format_args!($($arg)*));
            boot_println!($console, "boot: {}", format_args!($($arg)*));
    };
}

macro_rules! kernel_log {
    ($($arg:tt)*) => {
        let text = format!("kernel: {}\n", format_args!($($arg)*));
        debug_println!("{}", text);
        let vfs = VFS.get().unwrap();
        let root = vfs.root.clone();
        let console = vfs.traverse_fs(root, "/dev/console").unwrap();
        vfs.write(console, 0, text.as_bytes()).unwrap();
    };
}

static VFS: OnceCell<VirtualFileSystem> = OnceCell::uninit();

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    unsafe { *&raw mut panic::PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }

    interrupts::init_idt();

    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Expected recursive index");

    let (mut mapper, mut pmm) = unsafe { memory::init(physical_offset, &boot_info.memory_regions) };

    let cpu = Box::leak(Box::new(unsafe { PerCpu::init_cpu() }));
    unsafe {
        cpu.init_gdt();
    }

    let mut console = Console::new(framebuffer);
    boot_log!(&mut console, "Console initialised.");

    boot_log!(&mut console, "Initialising APIC timer...");
    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };
    let mut lapic = unsafe { Lapic::new(&mut mapper, &mut pmm, 0xff) };
    lapic.configure_timer(0x31, 10000, lapic::TimerDivideConfig::DivideBy16);
    boot_log!(&mut console, "APIC timer initialised.");

    boot_log!(&mut console, "Initialising VFS...");
    VFS.init_once(|| {
        let mut vfs = VirtualFileSystem::new();
        let devfs = Devfs::new(console, 1);
        let initrd = Initrd::from_files(2, vec![("hello_world.txt", "Hello from initrd!")]);
        vfs.mount(1, Box::new(devfs), "dev", 0).unwrap();
        vfs.mount(2, Box::new(initrd), "init", 0).unwrap();
        vfs
    });
    kernel_log!("VFS initialised");

    kernel_log!("Allocating userspace...");
    let user_process = unsafe {
        UserProcess::new(
            &mut mapper,
            &mut pmm,
            VirtAddr::new(0x400000),
            &[0xEB, 0xFE],
            VirtAddr::new(0x0000_7fff_ffff_0000),
            &[0; 0x1000],
        )
    };
    kernel_log!("Allocated userspace.");
    kernel_log!("Switching to userspace...");
    user_process.switch();
    kernel_log!("Returned to kernel?");
    loop {
        hlt();
    }
}
