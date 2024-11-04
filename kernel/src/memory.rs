use bootloader_api::info::MemoryRegions;
use core::ptr::slice_from_raw_parts_mut;
use linked_list_allocator::LockedHeap;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::{OffsetPageTable, PhysFrame};
use x86_64::{PhysAddr, VirtAddr};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();



/// # Safety
/// Can only be called once. Physical offset must be correct
pub unsafe fn init(physical_offset: u64, memory_regions: &'static MemoryRegions) -> (OffsetPageTable<'static>, PhysicalMemoryManager<'static>) {
    let mapper = init_page_table(physical_offset);

    let pmm = PhysicalMemoryManager::new(&memory_regions, VirtAddr::new(physical_offset));

    (mapper, pmm)
}

fn init_page_table(physical_offset: u64) -> OffsetPageTable<'static> {
    let physical_offset = VirtAddr::new(physical_offset);

    let (l4_page_table_phys, _) = Cr3::read();

    let l4_page_table_addr = physical_offset + l4_page_table_phys.start_address().as_u64();
    let l4_page_table = l4_page_table_addr.as_mut_ptr();

    unsafe { OffsetPageTable::new(&mut *l4_page_table, physical_offset) }
}


#[derive(Debug)]
pub struct PhysicalMemoryManager<'a> {
    memory_regions: &'static MemoryRegions,
    bitmap: &'a mut [u64], // 0 for free, 1 for used
    physical_offset: VirtAddr
}

impl<'a> PhysicalMemoryManager<'a> {
    fn set_frame(&mut self, frame: PhysFrame) {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            |= 1 << frame.start_address().as_u64() % (4096 * 64);
    }

    fn clear_frame(&mut self, frame: PhysFrame) {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            &= !(1 << frame.start_address().as_u64() % (4096 * 64));
    }

    fn test_frame(&self, frame: PhysFrame) -> bool {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            & 1 << frame.start_address().as_u64() % (4096 * 64) > 0
    }

    fn new(memory_regions: &'static MemoryRegions, physical_offset: VirtAddr) -> Self {
        let highest_address = memory_regions.iter()
            .map(|region| region.end)
            .max()
            .unwrap();

        // This trick rounds up instead of down
        let region_size: usize = ((highest_address + 4096 * 8 - 1) / (4096 * 8)) as usize;

        let bitmap_region = memory_regions.iter()
            .filter(|region| region.end - region.start >= region_size as u64)
            .next().unwrap();

        // TODO: make memory regions --> bitmap

        let bitmap = slice_from_raw_parts_mut((physical_offset.as_u64() + bitmap_region.start) as *mut u64, region_size / 8);

        let bitmap = unsafe { &mut *bitmap };

        let mut pmm = PhysicalMemoryManager {
            memory_regions,
            bitmap,
            physical_offset
        };

        let bitmap_range = PhysFrameRange {
            start: PhysFrame::containing_address(PhysAddr::new(bitmap_region.start)),
            end: PhysFrame::containing_address(PhysAddr::new(bitmap_region.end).align_up(4096u64) + 1),
        };

        for frame in bitmap_range {
            pmm.set_frame(frame);
        }

        pmm
    }
}


