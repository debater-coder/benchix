use alloc::vec;
use alloc::vec::Vec;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use core::fmt;
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};
use x86_64::instructions::port::Port;

const SIZE: RasterHeight = RasterHeight::Size32;

/// Internal struct used by console to store framebuffer
struct Framebuffer {
    framebuffer_info: FrameBufferInfo,
    raw_framebuffer: &'static mut [u8],
}

pub struct Console {
    characters: Vec<u8>,
    framebuffer: Framebuffer,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
    offset: usize,
}

impl Console {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let framebuffer = Framebuffer {
            framebuffer_info: framebuffer.info().clone(),
            raw_framebuffer: framebuffer.buffer_mut(),
        };
        let (width, height) = (
            framebuffer.framebuffer_info.width,
            framebuffer.framebuffer_info.height,
        );
        let (rows, cols) = (height / Self::char_height(), width / Self::char_width());
        let mut console = Console {
            rows,
            cols,
            offset: 0,
            characters: vec![b' '; rows * cols],
            framebuffer,
            row: 0,
            col: 0,
        };
        console.full_redraw();
        console
    }

    fn char_mut(&mut self, row: usize, col: usize) -> &mut u8 {
        &mut self.characters[(row * self.cols + col + self.offset) % (self.rows * self.cols)]
    }

    fn char_ref(&self, row: usize, col: usize) -> &u8 {
        &self.characters[(row * self.cols + col + self.offset) % (self.rows * self.cols)]
    }

    pub fn read(&mut self, _buf: &[u8]) -> usize {
        unimplemented!()
    }

    fn newline(&mut self) {
        if self.row >= (self.rows - 1) {
            self.offset = (self.offset + self.cols) % (self.rows * self.cols); // Scroll down
                                                                               // Clear last row
            for x in 0..self.cols {
                *self.char_mut(self.rows - 1, x) = b' ';
            }
            self.full_redraw();
        } else {
            self.row += 1;
        }
        self.col = 0;
    }

    fn full_redraw(&mut self) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.update_character(row, col);
            }
        }
    }

    pub fn char_width() -> usize {
        get_raster_width(FontWeight::Regular, SIZE)
    }

    pub fn char_height() -> usize {
        SIZE.val()
    }

    fn update_character(&mut self, row: usize, col: usize) {
        let character_width = get_raster_width(FontWeight::Regular, SIZE);

        let x = col * character_width;
        let y = SIZE.val() * row;

        let raster = get_raster(*self.char_ref(row, col) as char, FontWeight::Regular, SIZE)
            .unwrap()
            .raster();

        for (row_i, row) in raster.iter().enumerate() {
            for (col_i, pixel) in row.iter().enumerate() {
                let info = self.framebuffer.framebuffer_info;
                let x = x + col_i;
                let y = y + row_i;
                let base = (y * info.stride + x) * info.bytes_per_pixel;
                self.framebuffer.raw_framebuffer[base] = *pixel;
                self.framebuffer.raw_framebuffer[base + 1] = *pixel;
                self.framebuffer.raw_framebuffer[base + 2] = *pixel;
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> usize {
        for byte in buf {
            match byte {
                b'\x08' => {
                    self.col -= 1;
                    *self.char_mut(self.row, self.col) = b' ';
                    self.update_character(self.row, self.col);
                }
                b'\n' => {
                    self.newline();
                }
                _ => {
                    *self.char_mut(self.row, self.col) = *byte;
                    self.update_character(self.row, self.col);

                    if self.col == self.cols - 1 {
                        self.newline()
                    } else {
                        self.col += 1;
                    }
                }
            }
        }

        buf.len()
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s.as_bytes());
        Ok(())
    }
}

#[macro_export]
macro_rules! boot_print {
    ($console:expr, $($arg:tt)*) => (<Console as core::fmt::Write>::write_fmt($console, format_args!($($arg)*)).unwrap(););
}

#[macro_export]
macro_rules! boot_println {
    ($console:expr) => ($crate::boot_print!($console, "\n"));
    ($console:expr, $($arg:tt)*) => ($crate::boot_print!($console, "{}\n", format_args!($($arg)*)));
}

/// This is an example of how not to write hardware interfaces
pub struct DebugCons;

impl fmt::Write for DebugCons {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            for c in s.as_bytes() {
                Port::new(0xe9).write(*c);
            }
        }

        Ok(())
    }
}

#[macro_export]
macro_rules! debug_print {
    ($($arg:tt)*) => (<crate::console::DebugCons as core::fmt::Write>::write_fmt(&mut crate::console::DebugCons {}, format_args!($($arg)*)));
}

#[macro_export]
macro_rules! debug_println {
    () => {
        let _ = $crate::debug_print!("\n");
    };
    ($($arg:tt)*) => {
        let _ = $crate::debug_print!("{}\n", format_args!($($arg)*));
    };
}
