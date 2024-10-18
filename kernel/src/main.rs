#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
mod console;
mod interrupts;
mod gdt;

use core::panic::PanicInfo;
use bootloader_api::info::FrameBuffer;
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


bootloader_api::entry_point!(kernel_main);
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
    boot_println!(&mut console, "{:?}", boot_info.physical_memory_offset);

    unsafe {*(0xdeadbeef as *const i32)};

    boot_println!(&mut console, "Boot complete!");
    loop {}
}