#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
extern crate alloc;
use core::fmt::Write;

mod console;
mod interrupts;
mod gdt;
mod memory;

use crate::console::Console;
use alloc::fmt;
use bootloader_api::config::Mapping;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use bootloader_api::BootloaderConfig;
use core::panic::PanicInfo;
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};
use x86_64::instructions::hlt;
use x86_64::structures::paging::PageTableIndex;

struct PanicConsole {
    x: usize,
    y: usize,
    frame_buffer: &'static mut FrameBuffer
}

impl PanicConsole {
    fn new_line(x: &mut usize, y: &mut usize, info: FrameBufferInfo) {
        if *y < info.height - 32 {
            *y += 32;
            *x = 0;
        } else {
            loop {
                hlt();
            }
        }
    }
}

impl Write for PanicConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let info = self.frame_buffer.info().clone();
        let buffer = self.frame_buffer.buffer_mut();

        for byte in s.as_bytes() {
            match byte {
                b'\n' => {
                    Self::new_line(&mut self.x, &mut self.y, info);
                },
                _ => {
                    let width = get_raster_width(FontWeight::Regular, RasterHeight::Size32);
                    if self.x + width >= info.width {
                        Self::new_line(&mut self.x, &mut self.y, info);
                    }

                    let raster = get_raster(*byte as char, FontWeight::Regular, RasterHeight::Size32)
                        .unwrap_or_else(|| {loop {hlt()}})
                        .raster();

                    for (row_i, row) in raster.iter().enumerate() {
                        for (col_i, pixel) in row.iter().enumerate() {
                            let y = self.y + row_i;
                            let x = self.x + col_i;

                            let base = (y * info.stride + x) * info.bytes_per_pixel;
                            buffer[base] = *pixel;
                            buffer[base + 1] = *pixel;
                            buffer[base + 2] = *pixel;
                        }
                    }
                    self.x += width;
                }
            }
        }

        Ok(())
    }
}

static mut PANIC_FRAMEBUFFER: Option<*mut FrameBuffer> = None;
/// This function is called on panic.
/// On kernel panic, it is best to use as little existing infrastructure as possible as it may be
/// corrupted. This panic function is responsible for showing the panic info which was passed to it.
/// In order to avoid relying on the filesystem (to access the console), the panic handler instead
/// reinitialises the console from the framebuffer. This would normally be a violation of no mutable
/// aliasing rules, so to remain safe the panic handler is responsible for terminating all other
/// code running in the system, so it can have complete control without any rogue threads interfering.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(framebuffer) = unsafe { PANIC_FRAMEBUFFER } {
        let framebuffer = unsafe {&mut *framebuffer };

        {
            let (info, buffer) = (framebuffer.info().clone(), framebuffer.buffer_mut());

            for x in 0..info.width {
                for y in 0..info.height {
                    let base = (y * info.stride + x) * info.bytes_per_pixel;
                    buffer[base] = 0;
                    buffer[base + 1] = 0;
                    buffer[base + 2] = 0;
                }
            }
        }

        let mut console = PanicConsole {
            x: 0,
            y: 0,
            frame_buffer: framebuffer
        };

        let _ = write!(&mut console, "panicked: {}", info);
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

    let mut console= Console::new(framebuffer);

    for i in 0..=100 {
        boot_print!(&mut console, "{}", i);
    }

    boot_println!(&mut console);
    boot_println!(&mut console, "Boot complete!");
    loop {
        hlt();
    }
}