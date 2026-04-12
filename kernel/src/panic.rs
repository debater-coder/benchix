use core::arch::asm;
use core::panic::PanicInfo;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use alloc::fmt;
use alloc::slice;
use noto_sans_mono_bitmap::get_raster;
use x86_64::VirtAddr;
use x86_64::instructions;
use x86_64::instructions::interrupts;

use noto_sans_mono_bitmap::FontWeight;
use noto_sans_mono_bitmap::RasterHeight;

use noto_sans_mono_bitmap::get_raster_width;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::structures::paging::Translate;

use core::fmt::Write;

use x86_64::instructions::hlt;

use bootloader_api::info::FrameBufferInfo;

use bootloader_api::info::FrameBuffer;

use crate::elf::Elf;
use crate::elf::SymbolTableEntry;
use crate::memory;

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
                            .unwrap_or_else(|| {
                                loop {
                                    hlt()
                                }
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

pub static mut PANIC_FRAMEBUFFER: Option<*mut FrameBuffer> = None;
static PANICKING: AtomicBool = AtomicBool::new(false);
pub static PHYS_MEM_OFFSET: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_LEN: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_ADDR: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_IMAGE_OFFSET: AtomicU64 = AtomicU64::new(0);

pub struct PanicWriter {
    buffer: [u8; 65536],
    cursor: usize,
}

#[derive(Clone)]
#[repr(C)]
pub struct StackFrame {
    rbp: *const StackFrame,
    rip: u64,
}

impl StackFrame {
    fn from_current() -> Self {
        let rbp: u64;
        unsafe { asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags)) }

        StackFrame {
            rbp: rbp as *const _,
            rip: instructions::read_rip().as_u64(),
        }
    }
}

struct StackWalker {
    current_frame: StackFrame,
}

impl StackWalker {
    fn new(frame: StackFrame) -> Self {
        StackWalker {
            current_frame: frame,
        }
    }
}

impl Iterator for StackWalker {
    type Item = StackFrame;

    fn next(&mut self) -> Option<Self::Item> {
        let phys_offset = PHYS_MEM_OFFSET.load(Ordering::SeqCst);
        if phys_offset == 0 {
            return None;
        }

        let page_table = memory::init_page_table(phys_offset);

        // Check rbp points to kernel space
        if self.current_frame.rbp as u64 & (1 << 63) == 0 {
            return None;
        }

        // Ensure valid mapping to not page fault
        if let Some(_) = page_table.translate_addr(VirtAddr::from_ptr(self.current_frame.rbp)) {
            self.current_frame = unsafe { (*self.current_frame.rbp).clone() };
            Some(self.current_frame.clone())
        } else {
            None
        }
    }
}

impl Write for PanicWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        debug_print!("{}", s);
        self.buffer[self.cursor..self.cursor + s.len()].copy_from_slice(s.as_bytes());
        self.cursor += s.len();
        Ok(())
    }
}

struct Symbol<'a> {
    name: &'a str,
    addr: u64,
}

fn find_closest_symbol<'a>(
    elf: &'a Elf,
    addr: u64,
    symbols: &'a [SymbolTableEntry],
    strtab: u32,
) -> Option<Symbol<'a>> {
    let offset = KERNEL_IMAGE_OFFSET.load(Ordering::SeqCst);
    let closest_symbol = symbols
        .iter()
        .filter_map(|symbol| {
            if let Some(diff) = addr.checked_sub(symbol.st_value + offset) {
                Some((symbol, diff))
            } else {
                None
            }
        })
        .min_by_key(|(_, diff)| *diff)
        .and_then(|(symbol, _)| Some(symbol));

    if let Some(symbol) = closest_symbol {
        let name = elf
            .lookup_strtab(strtab, symbol.st_name)
            .ok()
            .and_then(|inner| inner);
        if let Some(name) = name {
            Some(Symbol {
                name,
                addr: symbol.st_value + offset,
            })
        } else {
            None
        }
    } else {
        None
    }
}

impl PanicWriter {
    pub fn new() -> Self {
        interrupts::disable();

        // Halt on double panic
        if PANICKING.swap(true, Ordering::SeqCst) {
            loop {
                hlt();
            }
        }

        PanicWriter {
            buffer: [0; _],
            cursor: 0,
        }
    }

    pub fn print_stack_trace(&mut self, frame: StackFrame) {
        writeln!(self, "Call trace:").ok();

        let walker = StackWalker::new(frame);

        let elf = Elf::new(unsafe {
            slice::from_raw_parts(
                (VirtAddr::new(PHYS_MEM_OFFSET.load(Ordering::SeqCst))
                    + KERNEL_ADDR.load(Ordering::SeqCst))
                .as_ptr(),
                KERNEL_LEN.load(Ordering::SeqCst) as usize,
            )
        })
        .ok();

        let res = match &elf {
            Some(elf) => elf.get_symbols().ok(),
            None => None,
        };

        for (i, frame) in walker.take(48).enumerate() {
            let closest_symbol = match (&elf, &res) {
                (Some(elf), Some((symbols, strtab))) => {
                    find_closest_symbol(elf, frame.rip, symbols, *strtab)
                }
                _ => None,
            };

            if let Some(symbol) = closest_symbol {
                writeln!(
                    self,
                    "#{} [0x{:x}] <{:#}+{}>",
                    i,
                    frame.rip as u64,
                    rustc_demangle::demangle(symbol.name),
                    frame.rip - symbol.addr
                )
                .ok();
            } else {
                writeln!(self, "#{} [0x{:x}] <unknown>", i, frame.rip as u64).ok();
            }
        }
    }

    pub fn finish(&mut self) -> ! {
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

            let _ = unsafe { console.write_str(str::from_utf8_unchecked(self.buffer.as_slice())) };
        }

        loop {
            hlt();
        }
    }
}

/// This function is called on panic.
/// On kernel panic, it is best to use as little existing infrastructure as possible as it may be
/// corrupted. This panic function is responsible for showing the panic info which was passed to it.
/// In order to avoid relying on the filesystem (to access the console), the panic handler instead
/// reinitialises the console from the framebuffer. This would normally be a violation of no mutable
/// aliasing rules, so to remain safe the panic handler is responsible for terminating all other
/// code running in the system, so it can have complete control without any rogue threads interfering.
#[panic_handler]
#[cfg(not(test))]
pub(crate) fn panic(info: &PanicInfo) -> ! {
    let mut panic_writer = PanicWriter::new();

    writeln!(&mut panic_writer, "=== KERNEL PANIC ===\n").ok();
    writeln!(&mut panic_writer, "panicked: {}\n", info).ok();
    panic_writer.print_stack_trace(StackFrame::from_current());
    writeln!(&mut panic_writer, "panicked: {}\n", info).ok();
    panic_writer.finish();
}
