use core::fmt;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};

const COLS: usize = 80;
const ROWS: usize = 24;
const SIZE: RasterHeight = RasterHeight::Size32;

/// Internal struct used by console to store framebuffer
struct Framebuffer {
    framebuffer_info: FrameBufferInfo,
    raw_framebuffer: &'static mut [u8],
}

pub struct Console {
    characters: [[u8; COLS]; ROWS],
    framebuffer: Framebuffer,
    row: usize,
    col: usize
}

impl Console {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let mut console = Console {
            characters: [[b' '; COLS]; ROWS],
            framebuffer: Framebuffer {
                framebuffer_info: framebuffer.info().clone(),
                raw_framebuffer: framebuffer.buffer_mut(),
            },
            row: 0,
            col: 0,
        };
        console.full_redraw();
        console
    }

    pub fn read(&mut self, _buf: &[u8]) -> usize {
        unimplemented!()
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row == ROWS - 1 {
            for row_idx in 0..ROWS-1 {
                let next_row = &self.characters[row_idx + 1].clone();
                // Fill row with contents of row below
                self.characters[row_idx].copy_from_slice(next_row);
            }
            self.characters[ROWS - 1].fill(b' '); // Clear last row
            self.full_redraw();
        } else {
            self.row += 1;
        }
    }

    fn full_redraw(&mut self) {
        for row in 0..ROWS {
            for col in 0..COLS {
                self.update_character(row, col);
            }
        }
    }

    fn update_character(&mut self, row: usize, col: usize) {
        let character_width = get_raster_width(FontWeight::Regular, SIZE);

        let x = col * character_width;
        let y = SIZE.val() * row;

        let raster = get_raster(self.characters[row][col] as char, FontWeight::Regular, SIZE)
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
                    self.characters[self.row][self.col] = b' ';
                    self.update_character(self.row, self.col);
                }
                b'\n' => {
                    self.newline();
                }
                _ => {
                    self.characters[self.row][self.col] = *byte;
                    self.update_character(self.row, self.col);

                    if self.col == COLS - 1 {
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
    ($console:expr, $($arg:tt)*) => (<Console as core::fmt::Write>::write_fmt($console, format_args!($($arg)*)));
}

#[macro_export]
macro_rules! boot_println {
    ($console:expr) => ($crate::boot_print!($console, "\n"));
    ($console:expr, $($arg:tt)*) => ($crate::boot_print!($console, "{}\n", format_args!($($arg)*)));
}
