#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
mod console;
mod interrupts;
mod gdt;

use core::panic::PanicInfo;
use crate::console::CONSOLE;

/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    if CONSOLE.get().is_some() {
        kprintln!("panicked: {}", _info);
    }
    loop {}
}

#[allow(unconditional_recursion)]
fn stack_overflow() {
    stack_overflow(); // for each recursion, the return address is pushed
}


bootloader_api::entry_point!(kernel_main);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    console::init(framebuffer);

    kprintln!("benchix kernel is booting\n");

    kprint!("Loading GDT and IDT... ");
    gdt::init();
    interrupts::init_idt();
    kprintln!("done.");

    kprintln!("Boot complete!");
    loop {}
}