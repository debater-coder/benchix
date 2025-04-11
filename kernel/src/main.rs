#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::arch::asm;
use core::fmt::Write;
use filesystem::devfs::Devfs;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use lapic::Lapic;
use memory::PhysicalMemoryManager;
use x86_64::registers::model_specific::Msr;
use x86_64::structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags};
use x86_64::VirtAddr;

mod console;
mod filesystem;
mod gdt;
mod interrupts;
mod lapic;
mod memory;

use crate::console::Console;
use alloc::fmt;
use bootloader_api::config::Mapping;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use bootloader_api::BootloaderConfig;
use core::panic::PanicInfo;
use noto_sans_mono_bitmap::{get_raster, get_raster_width, FontWeight, RasterHeight};
use x86_64::instructions::hlt;

struct PanicConsole {
    x: usize,
    y: usize,
    frame_buffer: &'static mut FrameBuffer,
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

pub const HEAP_START: u64 = 0x_ffff_9000_0000_0000;
pub const KERNEL_STACK_START: u64 = 0xffff_f700_0000_0000;
pub const KERNEL_STACK_SIZE: u64 = 80 * 1024; // 80 Kb
pub const LAPIC_START_VIRT: u64 = 0xffff_8fff_ffff_0000;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.kernel_stack_size = KERNEL_STACK_SIZE;
    config.mappings.kernel_stack = Mapping::FixedAddress(KERNEL_STACK_START);
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0xffff_e000_0000_0000)); // 16 TiB of RAM ought to be enough for anybody
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);
    config.mappings.dynamic_range_end = Some(0xffff_8fff_fffe_ffff);
    config
};

bootloader_api::entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    let framebuffer = boot_info.framebuffer.as_mut().unwrap();
    unsafe { *&raw mut PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }

    gdt::init();
    interrupts::init_idt();

    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Expected recursive index");

    let (mut mapper, mut pmm) = unsafe { memory::init(physical_offset, &boot_info.memory_regions) };

    let mut console = Console::new(framebuffer);

    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };
    let mut lapic = unsafe { Lapic::new(&mut mapper, &mut pmm, 0xff) };
    lapic.configure_timer(0x31, 0xffffff, lapic::TimerDivideConfig::DivideBy16);
    x86_64::instructions::interrupts::enable();
    boot_println!(&mut console, "Boot complete!");

    let mut vfs = VirtualFileSystem::new();
    let devfs = Devfs::new(console, 1);
    vfs.mount(1, Box::new(devfs), "dev", 0);

    for entry in vfs.readdir(vfs.root.clone()).unwrap() {
        debug_println!(
            "name: {}, (dev, inode): ({}, {})",
            entry.name,
            entry.dev,
            entry.inode
        );
    }

    for entry in vfs
        .readdir(vfs.traverse_fs(vfs.root.clone(), "/dev/").unwrap())
        .unwrap()
    {
        debug_println!(
            "name: {}, (dev, inode): ({}, {})",
            entry.name,
            entry.dev,
            entry.inode
        );
    }

    vfs.write(
        vfs.traverse_fs(vfs.root.clone(), "/dev/console").unwrap(),
        0,
        &"Hello, from /dev/console".as_bytes(),
    )
    .unwrap();

    // unsafe {
    //     // Allocates user code
    //     let user_addr = VirtAddr::new(0x400000);
    //     allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(user_addr));
    //     user_addr.as_mut_ptr::<u16>().write(0xFEEB); // Infinite loop

    //     // Allocates user stack
    //     let stack_addr = VirtAddr::new(0x0000_7fff_ffff_0000);
    //     allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(stack_addr));

    //     x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
    //     asm!(
    //         "mov rsp, 0x00007fffffffffff", // Stacks grow downwards
    //         "mov r11, 0x0202",             // Bit 9 is set, thus interrupts are enabled
    //         "mov rcx, 0x400000",
    //         "sysretq"
    //     );
    // }

    loop {
        hlt();
    }
}

unsafe fn allocate_user_page(
    mapper: &mut OffsetPageTable,
    pmm: &mut PhysicalMemoryManager,
    page: Page,
) {
    mapper
        .map_to(
            page,
            pmm.allocate_frame().expect("Could not allocate frame"),
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            pmm,
        )
        .unwrap()
        .flush();
}
