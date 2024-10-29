use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTable, PageTableFlags, PageTableIndex, PhysFrame, RecursivePageTable, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

pub const HEAP_START: u64 = 0x_4444_4444_0000;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const INITIAL_HEAP_SIZE: u64 = 4 * 1024;


/// # Safety
/// Can only be called once.
/// # Panics
/// If recursive index is invalid.
pub unsafe fn init(recursive_index: PageTableIndex, memory_regions: &'static MemoryRegions) -> (RecursivePageTable<'static>, LinearFrameAllocator) {
    let mut page_table = init_page_table(recursive_index);
    let mut pmm = LinearFrameAllocator::new(memory_regions);

    init_with_heap(&mut pmm, &mut page_table);

    (page_table, pmm)
}

fn init_page_table(recursive_index: PageTableIndex) -> RecursivePageTable<'static> {
    let recursive_index: u64 = recursive_index.into();
    let sign = (recursive_index & 0b1000000000) & 0o177777 << 48; // Extend 9th bit of recursive
    let page_table_address = sign | (recursive_index << 39) | (recursive_index << 30) | (recursive_index << 21) | (recursive_index << 12);
    let page_table_pointer = page_table_address as *mut PageTable;
    RecursivePageTable::new(unsafe {&mut *page_table_pointer}).unwrap()
}

pub unsafe fn init_with_heap(frame_allocator: &mut impl FrameAllocator<Size4KiB>, mapper: &mut impl Mapper<Size4KiB>) {
    init_with_heap_inner(frame_allocator, mapper)
}

fn init_with_heap_inner(frame_allocator: &mut impl FrameAllocator<Size4KiB>, mapper: &mut (impl Mapper<Size4KiB> + Sized)) {

    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + INITIAL_HEAP_SIZE - 1u64;
    let page_range = Page::range_inclusive(
        Page::containing_address(heap_start),
        Page::containing_address(heap_end),
    );

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .expect("Failed to initialise heap");
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator).expect("Failed to initialise heap").flush();
        }
    }

    unsafe { ALLOCATOR.lock().init(heap_start.as_mut_ptr(), INITIAL_HEAP_SIZE as usize) };
}



pub struct LinearFrameAllocator {
    next: usize,
    memory_regions: &'static MemoryRegions
}

impl LinearFrameAllocator {
    fn available_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let available_memory_regions = self
            .memory_regions
            .iter()
            .filter(|region| region.kind == MemoryRegionKind::Usable);

        let available_frames = available_memory_regions
            .clone()
            .map(|region| region.start..region.end)
            .flatten()
            .filter(|addr| (addr & 0xfff) == 0)
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)));

        available_frames
    }
    unsafe fn new(memory_regions: &'static MemoryRegions) -> Self {
        LinearFrameAllocator {
            next: 0,
            memory_regions,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for LinearFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.available_frames().nth(self.next);
        self.next += 1;
        frame
    }
}