#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
mod console;
mod interrupts;
mod gdt;

use core::panic::PanicInfo;
use crate::console::{Console};

/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // if CONSOLE.get().is_some() {
    //     kprintln!("panicked: {}", _info);
    // }
    loop {}
}


bootloader_api::entry_point!(kernel_main);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    let mut console = Console::new(framebuffer);

    boot_println!(&mut console, "benchix kernel is booting\n");

    boot_print!(&mut console, "Loading GDT and IDT... ");
    gdt::init();
    interrupts::init_idt();
    boot_println!(&mut console, "done.");

    // set up memory
    boot_println!(&mut console, "{:?}", boot_info.physical_memory_offset);

    boot_println!(&mut console, "Boot complete!");
    loop {}
}