use core::panic::PanicInfo;

use alloc::fmt;
use noto_sans_mono_bitmap::get_raster;

use noto_sans_mono_bitmap::FontWeight;
use noto_sans_mono_bitmap::RasterHeight;

use noto_sans_mono_bitmap::get_raster_width;

use core::fmt::Write;

use x86_64::instructions::hlt;

use bootloader_api::info::FrameBufferInfo;

use bootloader_api::info::FrameBuffer;

use crate::debug_println;

pub(crate) struct PanicConsole {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) frame_buffer: &'static mut FrameBuffer,
}

impl PanicConsole {
    pub(crate) fn new_line(x: &mut usize, y: &mut usize, info: FrameBufferInfo) {
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
                }
                _ => {
                    let width = get_raster_width(FontWeight::Regular, RasterHeight::Size32);
                    if self.x + width >= info.width {
                        Self::new_line(&mut self.x, &mut self.y, info);
                    }

                    let raster =
                        get_raster(*byte as char, FontWeight::Regular, RasterHeight::Size32)
                            .unwrap_or_else(|| loop {
                                hlt()
                            })
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

pub(crate) static mut PANIC_FRAMEBUFFER: Option<*mut FrameBuffer> = None;

/// This function is called on panic.
/// On kernel panic, it is best to use as little existing infrastructure as possible as it may be
/// corrupted. This panic function is responsible for showing the panic info which was passed to it.
/// In order to avoid relying on the filesystem (to access the console), the panic handler instead
/// reinitialises the console from the framebuffer. This would normally be a violation of no mutable
/// aliasing rules, so to remain safe the panic handler is responsible for terminating all other
/// code running in the system, so it can have complete control without any rogue threads interfering.
#[cfg(not(test))]
#[panic_handler]
pub(crate) fn panic(info: &PanicInfo) -> ! {
    debug_println!("panicked: {}", info);
    if let Some(framebuffer) = unsafe { PANIC_FRAMEBUFFER } {
        let framebuffer = unsafe { &mut *framebuffer };

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
            frame_buffer: framebuffer,
        };

        let _ = write!(&mut console, "panicked: {}", info);
    }

    loop {}
}
