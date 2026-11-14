use crate::debug_println;
use alloc::vec;
use alloc::vec::Vec;
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent, TraceType};
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use core::{arch::x86_64::_rdtsc, fmt, str::Utf8Error};
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};
use postcard::to_slice_cobs;
use x86_64::{
    VirtAddr,
    instructions::port::Port,
    structures::paging::{
        Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB, Translate, mapper::TranslateResult,
    },
};

const SIZE: RasterHeight = RasterHeight::Size32;

/// Internal struct used by console to store framebuffer
struct Framebuffer {
    framebuffer_info: FrameBufferInfo,
    raw_framebuffer: &'static mut [u8],
}

impl Framebuffer {
    fn set_write_combining(&mut self, mapper: &mut OffsetPageTable) {
        let base = VirtAddr::from_ptr(self.raw_framebuffer);
        for page in Page::range(
            Page::<Size4KiB>::containing_address(base),
            Page::containing_address(base + self.framebuffer_info.byte_len as u64),
        ) {
            let translate = mapper.translate(page.start_address());
            let TranslateResult::Mapped {
                frame: _,
                offset: _,
                mut flags,
            } = translate
            else {
                panic!("expected framebuffer to be mapped, {:?}", translate);
            };

            // This selects PAT entry 0b101 = 5, which is set to
            // WriteCombining in PerCpu
            flags.set(PageTableFlags::PAT_4KIB_PAGE, true); // PAT = 1
            flags.set(PageTableFlags::NO_CACHE, false); // PCD = 0
            flags.set(PageTableFlags::WRITE_THROUGH, true); // PWT = 1

            unsafe {
                mapper.update_flags(page, flags).unwrap().flush();
            };
        }
    }
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
    pub fn new(framebuffer: &'static mut FrameBuffer, mapper: &mut OffsetPageTable) -> Self {
        let mut framebuffer = Framebuffer {
            framebuffer_info: framebuffer.info(),
            raw_framebuffer: framebuffer.buffer_mut(),
        };

        framebuffer.set_write_combining(mapper);

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

    fn newline(&mut self) -> bool {
        let old_row = self.row;
        let old_col = self.col;
        let mut need_redraw = false;
        if self.row >= (self.rows - 1) {
            self.offset = (self.offset + self.cols) % (self.rows * self.cols); // Scroll down
            // Clear last row
            for x in 0..self.cols {
                *self.char_mut(self.rows - 1, x) = b' ';
            }
            need_redraw = true;
        } else {
            self.row += 1;
        }
        self.col = 0;

        self.update_character(old_row, old_col);

        need_redraw
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
        let is_cursor = if row == self.row && col == self.col {
            0xff
        } else {
            0
        };

        let character_width = get_raster_width(FontWeight::Regular, SIZE);

        let x = col * character_width;
        let y = SIZE.val() * row;

        let raster = get_raster(*self.char_ref(row, col) as char, FontWeight::Regular, SIZE)
            .unwrap_or(get_raster('?', FontWeight::Regular, SIZE).unwrap())
            .raster();

        for (row_i, row) in raster.iter().enumerate() {
            for (col_i, pixel) in row.iter().enumerate() {
                let info = self.framebuffer.framebuffer_info;
                let x = x + col_i;
                let y = y + row_i;
                let base = (y * info.stride + x) * info.bytes_per_pixel;
                self.framebuffer.raw_framebuffer[base] = *pixel ^ is_cursor;
                self.framebuffer.raw_framebuffer[base + 1] = *pixel ^ is_cursor;
                self.framebuffer.raw_framebuffer[base + 2] = *pixel ^ is_cursor;
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> usize {
        let mut need_redraw = false;

        for byte in buf {
            match byte {
                b'\x08' => {
                    self.col -= 1;
                    *self.char_mut(self.row, self.col) = b' ';
                    self.update_character(self.row, self.col + 1);
                    self.update_character(self.row, self.col);
                }
                b'\n' => {
                    need_redraw |= self.newline();
                }
                _ => {
                    *self.char_mut(self.row, self.col) = *byte;

                    if self.col == self.cols - 1 {
                        need_redraw |= self.newline();
                    } else {
                        self.col += 1;
                        self.update_character(self.row, self.col - 1);
                        self.update_character(self.row, self.col);
                    }
                }
            }
        }
        if need_redraw {
            self.full_redraw();
        }
        buf.len()
    }
}

#[macro_export]
macro_rules! early_print {
    ($console:expr, $($arg:tt)*) => {
        let mut string: alloc::string::String = alloc::string::String::new();
        let _ = <alloc::string::String as core::fmt::Write>::write_fmt(&mut string, format_args!($($arg)*));
        $console.write(string.as_bytes());
    };
}

#[macro_export]
macro_rules! early_println {
    ($console:expr) => ($crate::early_print!($console, "\n"));
    ($console:expr, $($arg:tt)*) => ($crate::early_print!($console, "{}\n", format_args!($($arg)*)));
}

/// Writes a message to debug port (using the 0xe9 '-debugcon' protocol)
pub struct DebugCons {
    cursor: usize,
    buf: [u8; 3072],
}

impl Default for DebugCons {
    fn default() -> Self {
        Self {
            cursor: Default::default(),
            buf: [0u8; 3072],
        }
    }
}

pub fn trace_inner(trace_type: TraceType, calling_address: u64) {
    let mut out_buf = [0u8; MAX_MSG_SIZE];
    match to_slice_cobs(
        &TraceEvent {
            calling_address,
            trace_type,
            timestamp: unsafe { _rdtsc() },
        },
        &mut out_buf,
    ) {
        Ok(used) => unsafe {
            Port::new(0xe9).write(0u8); // COBS sentinel
            for c in used {
                Port::new(0xe9).write(*c);
            }
        },
        Err(postcard::Error::SerializeBufferFull) => {
            debug_println!("[WARNING] trace buffer full");
        }
        _ => {}
    }
}

#[inline(never)]
pub fn trace(trace_type: TraceType) {
    trace_inner(trace_type, core::intrinsics::return_address() as u64);
}

impl fmt::Write for DebugCons {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.cursor + s.len() > self.buf.len() {
            return Err(fmt::Error);
        }

        self.buf[self.cursor..self.cursor + s.len()].copy_from_slice(s.as_bytes());
        self.cursor += s.len();
        Ok(())
    }
}

impl DebugCons {
    #[inline(never)]
    pub fn flush(&mut self) -> Result<(), Utf8Error> {
        trace_inner(
            TraceType::Msg(str::from_utf8(&self.buf[..self.cursor])?),
            core::intrinsics::return_address() as u64,
        );

        Ok(())
    }
}

#[macro_export]
macro_rules! debug_print {
    ($($arg:tt)*) => {
        let mut dbg_port = Default::default();
        let _ = <$crate::console::DebugCons as core::fmt::Write>::write_fmt(&mut dbg_port, format_args!($($arg)*));
        let _ = dbg_port.flush();
    };
}

#[macro_export]
macro_rules! debug_println {
    () => {
        $crate::debug_print!("\n");
    };
    ($($arg:tt)*) => {
        $crate::debug_print!("{}\n", format_args!($($arg)*));
    };
}
