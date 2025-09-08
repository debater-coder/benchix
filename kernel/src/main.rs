#![feature(abi_x86_interrupt, new_zeroed_alloc, allocator_api)]
#![no_std]
#![no_main]
extern crate alloc;

use acpi::{AcpiTables, PlatformInfo};
use alloc::boxed::Box;
use conquer_once::spin::OnceCell;
use cpu::{Cpus, PerCpu};
use filesystem::devfs::Devfs;
use filesystem::ramdisk::Ramdisk;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use memory::PhysicalMemoryManager;
use spin::mutex::Mutex;
use user::UserProcess;
use x86_64::VirtAddr;

#[macro_use]
mod console;
mod acpi_handler;
mod apic;
mod cpu;
mod filesystem;
mod interrupts;
mod memory;
#[allow(dead_code, unused_imports)]
mod panic;
mod scheduler;
mod user;

use crate::console::Console;
use alloc::{slice, vec};

use bootloader_api::BootloaderConfig;
use bootloader_api::config::Mapping;

pub const HEAP_START: u64 = 0x_ffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_f700_0000_0000;
pub const KERNEL_STACK_SIZE: u64 = 80 * 1024; // 80 Kb
pub const LAPIC_START_VIRT: u64 = 0xffff_8fff_ffff_0000;
pub const IOAPIC_START_VIRT: u64 = 0xffff_a000_0000_0000;

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
pub static PMM: OnceCell<Mutex<PhysicalMemoryManager>> = OnceCell::uninit();

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    unsafe { *&raw mut panic::PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }

    interrupts::init_idt();

    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Expected recursive index");

    // The mapper will create kernel memory mappings, which will be frozen from after first process creation.
    // These mappings will be inherited by all future processes.
    let (mut mapper, pmm) = unsafe { memory::init(physical_offset, &boot_info.memory_regions) };
    PMM.get_or_init(|| Mutex::new(pmm));

    CPUS.init_once(|| Cpus::new(unsafe { PerCpu::init_cpu() }));
    unsafe {
        CPUS.get().unwrap().get_cpu().init_gdt();
    }

    let mut console = Console::new(framebuffer);
    early_log!(&mut console, "Console initialised.");

    early_log!(&mut console, "Parsing ACPI tables...");
    let acpi_tables = unsafe {
        AcpiTables::from_rsdp(
            acpi_handler::Handler {
                phys_offset: VirtAddr::new(physical_offset),
            },
            boot_info.rsdp_addr.into_option().unwrap() as usize,
        )
    }
    .unwrap();

    let platform_info = PlatformInfo::new(&acpi_tables).unwrap();
    debug_println!("Parsed ACPI tables: {:#?}", platform_info);

    early_log!(&mut console, "Initialising APIC devices...");

    early_log!(&mut console, "APIC timer initialised.");
    apic::enable(&mut mapper, &platform_info.interrupt_model);
    early_log!(&mut console, "Ramdisk size: {}", boot_info.ramdisk_len);

    early_log!(&mut console, "Initialising VFS...");
    let binary: &[u8] = unsafe {
        slice::from_raw_parts(
            VirtAddr::new(boot_info.ramdisk_addr.into_option().unwrap()).as_ptr(),
            boot_info.ramdisk_len as usize,
        )
    };
    VFS.init_once(|| {
        let mut vfs = VirtualFileSystem::new();
        let devfs = Devfs::init(console, 1);
        let ramdisk = unsafe { Ramdisk::from_tar(2, &binary) };
        vfs.mount(1, Box::new(devfs), "dev", 0).unwrap();
        vfs.mount(2, Box::new(ramdisk), "init", 0).unwrap();
        vfs
    });
    kernel_log!("VFS initialised");

    kernel_log!("Initialising scheduler");
    scheduler::init();
    kernel_log!("Scheduler initialised.");

    kernel_log!("Creating init process...");

    let init_process = UserProcess::new(mapper);
    kernel_log!("Init process created");

    let vfs = VFS.get().unwrap();
    let inode = vfs.traverse_fs(vfs.root.clone(), "/init/init").unwrap();

    let mut executable = vec![0; inode.size];

    vfs.read(inode, 0, executable.as_mut_slice()).unwrap();

    init_process
        .lock()
        .execve(
            executable.as_slice(),
            vec!["/init/init", "arg2", "arg3"],
            vec![],
        )
        .unwrap();

    kernel_log!("execve completed.");

    scheduler::enqueue(init_process.lock().thread.clone());

    kernel_log!("Yielding to scheduler");
    loop {
        scheduler::yield_execution();
    }
}
