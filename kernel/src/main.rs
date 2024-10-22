#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
mod console;
mod interrupts;
mod gdt;

use core::panic::PanicInfo;
use bootloader_api::BootloaderConfig;
use bootloader_api::config::Mapping;
use bootloader_api::info::{FrameBuffer, MemoryRegionKind};
use crate::console::{Console};

static mut PANIC_FRAMEBUFFER: Option<*mut FrameBuffer> = None;
/// This function is called on panic.
/// On kernel panic, it is best to use as little existing infrastructure as possible as it may be
/// corrupted. This panic function is responsible for showing the panic info which was passed to it.
/// In order to avoid relying on the filesystem (to access the console), the panic handler instead
/// reinitialises the console from the framebuffer. This would normally be a violation of no mutable
/// aliasing rules, so to remain safe the panic handler is responsible for terminating all other
/// code running in the system, so it can have complete control without any rogue threads interfering.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    if let Some(framebuffer) = unsafe { PANIC_FRAMEBUFFER } {
        let mut console = Console::new(unsafe { &mut *framebuffer });
            boot_println!(&mut console, "panicked: {}", _info);
    }

    loop {}
}

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.page_table_recursive = Some(Mapping::Dynamic);
    config
};


bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    unsafe { PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }
    let mut console = Console::new(framebuffer);

    boot_println!(&mut console, "benchix kernel is booting\n");

    boot_print!(&mut console, "Loading GDT and IDT... ");
    gdt::init();
    interrupts::init_idt();
    boot_println!(&mut console, "done.");

    // set up memory
    boot_println!(&mut console, "{:?}", boot_info.recursive_index);

    let memory_regions = boot_info.memory_regions.as_mut().iter().filter(|region| region.kind == MemoryRegionKind::Usable);
    for region in memory_regions {
        boot_println!(&mut console, "{:x?}", region);
    }

    boot_println!(&mut console, "Boot complete!");
    loop {}
}