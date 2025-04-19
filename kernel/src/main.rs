#![feature(naked_functions)]
#![feature(abi_x86_interrupt, new_zeroed_alloc)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use conquer_once::spin::OnceCell;
use cpu::{Cpus, PerCpu};
use filesystem::devfs::Devfs;
use filesystem::initrd::Initrd;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use lapic::Lapic;
use user::UserProcess;
use x86_64::registers::model_specific::Msr;
use x86_64::VirtAddr;

#[macro_use]
mod console;
mod cpu;
mod filesystem;
mod interrupts;
mod lapic;
mod memory;
mod panic;
mod user;

use crate::console::Console;
use alloc::{slice, vec};

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

macro_rules! early_log {
    ($console:expr, $($arg:tt)*) => {
            debug_println!("boot: {}", format_args!($($arg)*));
            early_println!($console, "boot: {}", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kernel_log {
    ($($arg:tt)*) => {
        let text = alloc::format!("kernel: {}\n", format_args!($($arg)*));
        debug_println!("{}", text);
        let vfs = $crate::VFS.get().unwrap();
        let root = vfs.root.clone();
        let console = <$crate::filesystem::vfs::VirtualFileSystem as $crate::filesystem::vfs::Filesystem>::traverse_fs(&vfs, root, "/dev/console").unwrap();
        <$crate::filesystem::vfs::VirtualFileSystem as $crate::filesystem::vfs::Filesystem>::write(&vfs, console, 0, text.as_bytes()).unwrap();
    };
}

pub static VFS: OnceCell<VirtualFileSystem> = OnceCell::uninit();
pub static CPUS: OnceCell<Cpus> = OnceCell::uninit();

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

    CPUS.init_once(|| Cpus::new(unsafe { PerCpu::init_cpu() }));
    unsafe {
        CPUS.get().unwrap().get_cpu().init_gdt();
    }

    let mut console = Console::new(framebuffer);
    early_log!(&mut console, "Console initialised.");

    early_log!(&mut console, "Initialising APIC timer...");
    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };
    let mut lapic = unsafe { Lapic::new(&mut mapper, &mut pmm, 0xff) };
    lapic.configure_timer(0x31, 10000, lapic::TimerDivideConfig::DivideBy16);
    early_log!(&mut console, "APIC timer initialised.");
    early_log!(&mut console, "Ramdisk size: {}", boot_info.ramdisk_len);

    early_log!(&mut console, "Initialising VFS...");
    VFS.init_once(|| {
        let mut vfs = VirtualFileSystem::new();
        let devfs = Devfs::new(console, 1);
        let initrd = Initrd::from_files(
            2,
            vec![("hello_world.txt", "Hello from initrd!".as_bytes())],
        );
        vfs.mount(1, Box::new(devfs), "dev", 0).unwrap();
        vfs.mount(2, Box::new(initrd), "init", 0).unwrap();
        vfs
    });
    kernel_log!("VFS initialised");

    kernel_log!("Allocating userspace...");
    let binary = unsafe {
        slice::from_raw_parts(
            VirtAddr::new(boot_info.ramdisk_addr.into_option().unwrap()).as_ptr(),
            boot_info.ramdisk_len as usize,
        )
    };

    let user_process = UserProcess::load_elf(&mut mapper, &mut pmm, binary).unwrap();

    kernel_log!("Allocated userspace.");
    kernel_log!("Switching to userspace...");
    CPUS.get().unwrap().get_cpu().switch(user_process);
    kernel_log!("Returned to kernel?");
    loop {
        hlt();
    }
}
