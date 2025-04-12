#![feature(abi_x86_interrupt, new_zeroed_alloc)]
#![no_std]
#![no_main]
extern crate alloc;

use alloc::boxed::Box;
use core::arch::asm;
use core::str;
use filesystem::devfs::Devfs;
use filesystem::initrd::Initrd;
use filesystem::vfs::{Filesystem, VirtualFileSystem};
use gdt::PerCpu;
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
mod panic;

use crate::console::Console;
use alloc::vec;
use bootloader_api::config::Mapping;
use bootloader_api::BootloaderConfig;
use x86_64::instructions::hlt;

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
    unsafe { *&raw mut panic::PANIC_FRAMEBUFFER = Some(&raw mut *framebuffer) }

    interrupts::init_idt();

    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Expected recursive index");

    let (mut mapper, mut pmm) = unsafe { memory::init(physical_offset, &boot_info.memory_regions) };

    let cpu = Box::leak(Box::new(unsafe { PerCpu::init_cpu() }));
    unsafe {
        cpu.init_gdt();
    }

    let mut console = Console::new(framebuffer);

    let mut apic_base_msr = Msr::new(0x1b);
    unsafe { apic_base_msr.write(apic_base_msr.read() | (1 << 11)) };
    let mut lapic = unsafe { Lapic::new(&mut mapper, &mut pmm, 0xff) };
    lapic.configure_timer(0x31, 0xffffff, lapic::TimerDivideConfig::DivideBy16);
    x86_64::instructions::interrupts::enable();
    boot_println!(&mut console, "Boot complete!");

    let mut vfs = VirtualFileSystem::new();
    let devfs = Devfs::new(console, 1);
    let initrd = Initrd::from_files(2, vec![("hello_world.txt", "Hello from initrd!")]);

    vfs.mount(1, Box::new(devfs), "dev", 0);
    vfs.mount(2, Box::new(initrd), "init", 0);

    unsafe {
        // Allocates user code
        let user_addr = VirtAddr::new(0x400000);
        allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(user_addr));
        user_addr.as_mut_ptr::<u16>().write(0xFEEB); // Infinite loop

        // Allocates user stack
        let stack_addr = VirtAddr::new(0x0000_7fff_ffff_0000);
        allocate_user_page(&mut mapper, &mut pmm, Page::containing_address(stack_addr));

        x86_64::instructions::interrupts::disable(); // To avoid handling interrupts with user stack
        asm!(
            "mov rsp, 0x00007fffffffffff", // Stacks grow downwards
            "mov r11, 0x0202",             // Bit 9 is set, thus interrupts are enabled
            "mov rcx, 0x400000",
            "sysretq"
        );
    }

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
