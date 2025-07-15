use crate::IOAPIC_START_VIRT;
use crate::PMM;
use x86_64::structures::paging::Mapper;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::structures::paging::Page;
use x86_64::structures::paging::PageTableFlags;
use x86_64::structures::paging::PhysFrame;
use x86_64::structures::paging::Size4KiB;
use x86_64::{PhysAddr, VirtAddr};

pub struct IoApic {
    ioregsel: &'static mut u32,
    iowin: &'static mut u32,
}

#[allow(dead_code)]
enum DestinationMode {
    Physical = 0,
    Logical = 1,
}

#[allow(dead_code)]
enum PinPolarity {
    ActiveHigh = 0,
    ActiveLow = 1,
}

impl IoApic {
    pub(crate) fn set_ioredtbl(&mut self, irq: u8, vector: u8, lapic_id: u8) {
        let low_offset = 0x10 + irq * 2;
        let high_offset = 0x10 + irq * 2 + 1;

        let ioredtbl = (self.read(low_offset) as u64) | ((self.read(high_offset) as u64) << 32);

        let delivery_mode = 0b000u8;

        let destination_mode = DestinationMode::Physical as u8;

        let pin_polarity = PinPolarity::ActiveHigh as u8;

        let ioredtbl = (ioredtbl & !0x0f0_0000_0001_efff)
            | (vector as u64)
            | (((delivery_mode & 0b111) as u64) << 8)
            | (((destination_mode & 0b1) as u64) << 11)
            | (((pin_polarity & 0b1) as u64) << 13)
            | (((lapic_id & 0xf) as u64) << 56);

        self.write(low_offset, ioredtbl as u32);
        self.write(high_offset, (ioredtbl >> 32) as u32)
    }

    fn read(&mut self, offset: u8) -> u32 {
        *self.ioregsel = offset as u32;
        *self.iowin
    }

    fn write(&mut self, offset: u8, value: u32) {
        *self.ioregsel = offset as u32;
        *self.iowin = value;
    }

    pub(crate) fn new(
        mapper: &mut OffsetPageTable<'static>,
        io_apic: &acpi::platform::interrupt::IoApic,
    ) -> Self {
        let base_address = io_apic.address;

        unsafe {
            mapper
                .map_to(
                    Page::<Size4KiB>::containing_address(VirtAddr::new(IOAPIC_START_VIRT)),
                    PhysFrame::containing_address(PhysAddr::new(base_address as u64)),
                    PageTableFlags::WRITABLE | PageTableFlags::PRESENT | PageTableFlags::NO_CACHE,
                    &mut *PMM.get().unwrap().lock(),
                )
                .unwrap()
                .flush();
        }

        let ioregsel: *mut u32 = VirtAddr::new(IOAPIC_START_VIRT).as_mut_ptr();
        let ioregsel = unsafe { &mut *ioregsel };

        let iowin: *mut u32 = VirtAddr::new((IOAPIC_START_VIRT) + 0x10).as_mut_ptr();
        let iowin = unsafe { &mut *iowin };

        IoApic { ioregsel, iowin }
    }
}
