#![no_std]
#![no_main]
mod console;

use core::panic::PanicInfo;

/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


bootloader_api::entry_point!(kernel_main);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    console::init(framebuffer);

    for i in 1..=100 {
        kprintln!("Hello, World {i}");
    }

    loop {}
}