#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


bootloader_api::entry_point!(kernel_main);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let mut framebuffer = boot_info.framebuffer.take().unwrap();
    let info = framebuffer.info().clone();
    let mut buffer = framebuffer.buffer_mut();

    for y in 0..info.height {
        for x in 0..info.width {
            buffer[(y * info.stride + x) * info.bytes_per_pixel] = ((x as f32 / info.width as f32) * 255f32) as u8; // blue
            buffer[(y * info.stride + x) * info.bytes_per_pixel + 1] = ((y as f32 / info.height as f32) * 255f32) as u8; // green
            buffer[(y * info.stride + x) * info.bytes_per_pixel + 2] = 128; // red
        }
    }

    loop {}
}