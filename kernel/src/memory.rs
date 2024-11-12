use core::fmt::{Display, Formatter};
use core::mem::zeroed;
use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use core::ptr::slice_from_raw_parts_mut;
use linked_list_allocator::LockedHeap;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, FrameDeallocator, OffsetPageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};
use crate::debug_println;

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
    bitmap: &'a mut [u64], // 0 for free, 1 for used
    physical_offset: VirtAddr
}

impl Display for PhysicalMemoryManager<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "Physical address: {:?}", self.physical_offset)?;
        writeln!(f)?;


        writeln!(f, "Bitmap physical address: base {:?} size {:?}", (self.bitmap.as_ptr() as u64) - self.physical_offset.as_u64(), self.bitmap.len())?;
        for (index, value) in self.bitmap.iter().enumerate() {
            if *value > 0 {
                writeln!(f, "{:0>16x}: {:0>64b}", index * 4096 * 64, value)?;
            }
        }

        Ok(())
    }
}

impl<'a> PhysicalMemoryManager<'a> {
    fn set_frame(&mut self, frame: PhysFrame) {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            |= 1 << (frame.start_address().as_u64() / 4096) % 64;
    }

    fn clear_frame(&mut self, frame: PhysFrame) {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            &= !(1 << (frame.start_address().as_u64() / 4096) % 64);
    }

    fn test_frame(&self, frame: PhysFrame) -> bool {
        self.bitmap[frame.start_address().as_u64() as usize / (4096 * 64)]
            & 1 << (frame.start_address().as_u64() / 4096) % 64 > 0
    }

    fn new(memory_regions: &'static MemoryRegions, physical_offset: VirtAddr) -> Self {
        let highest_address = memory_regions.iter()
            .map(|region| region.end)
            .max()
            .unwrap();

        // This trick rounds up instead of down
        let region_size: usize = ((highest_address + 4096 * 8 - 1) / (4096 * 8)) as usize;

        let bitmap_region = memory_regions.iter()
            .filter(|region| region.kind == MemoryRegionKind::Usable)
            .filter(|region| region.end - region.start >= region_size as u64)
            .next().unwrap();

        let bitmap = slice_from_raw_parts_mut((physical_offset.as_u64() + bitmap_region.start) as *mut u64, region_size / 8);

        let bitmap = unsafe { &mut *bitmap };

        for mem in &mut *bitmap {
            *mem = unsafe { zeroed::<u64>() };
        }

        let mut pmm = PhysicalMemoryManager {
            bitmap,
            physical_offset
        };

        let bitmap_range = PhysFrame::range_inclusive(
            PhysFrame::containing_address(PhysAddr::new(bitmap_region.start)),
            PhysFrame::containing_address(PhysAddr::new(bitmap_region.end - 1)), // End address is exclusive
        );


        for frame in bitmap_range {
            pmm.set_frame(frame);
        }

        for region in memory_regions.iter()
            .filter(|region| region.kind != MemoryRegionKind::Usable) {
            let frame_range = PhysFrame::range_inclusive(
                PhysFrame::containing_address(PhysAddr::new(region.start)),
                PhysFrame::containing_address(PhysAddr::new(region.end - 1)), // End address is exclusive
            );

            for frame in frame_range {
                pmm.set_frame(frame);
            }
        }

        pmm
    }
}

unsafe impl<'a> FrameAllocator<Size4KiB> for PhysicalMemoryManager<'a> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        for (idx, entry) in self.bitmap.iter().enumerate() {
            if *entry != u64::MAX {
                let frame = PhysFrame::containing_address(
                    PhysAddr::new((idx as u64 * 64 + entry.trailing_ones() as u64) * 4096)
                );

                self.set_frame(frame);

                return Some(frame)
            }
        }

        None
    }
}


impl<'a> FrameDeallocator<Size4KiB> for PhysicalMemoryManager<'a> {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.clear_frame(frame);
    }
}
