#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
extern crate alloc;

mod console;
mod interrupts;
mod gdt;
mod memory;

use alloc::vec;
use crate::console::Console;
use bootloader_api::config::Mapping;
use bootloader_api::info::FrameBuffer;
use bootloader_api::BootloaderConfig;
use core::panic::PanicInfo;
use x86_64::structures::paging::PageTableIndex;

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
        let mut characters = [b' '; 80 * 24];
        let mut console = Console::new(
            unsafe { &mut *framebuffer },
            characters.as_mut(),
            24,
            80
        );
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

    gdt::init();
    interrupts::init_idt();

    let recursive_index = boot_info.recursive_index.into_option().expect("Expected recursive index");
    let (mapper, frame_allocator) = unsafe { memory::init(PageTableIndex::new(recursive_index), &boot_info.memory_regions) };

    let (rows, cols) = (framebuffer.info().height / Console::char_height(), framebuffer.info().width / Console::char_width());

    let mut characters = vec![b' '; rows * cols].into_boxed_slice();

    let mut console= Console::new(framebuffer, characters.as_mut(), rows, cols);

    for i in 0..=100 {
        boot_print!(&mut console, "{}", i);
    }

    boot_println!(&mut console);

    boot_println!(&mut console, "Boot complete!");
    loop {}
}