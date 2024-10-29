use alloc::vec;
use alloc::vec::Vec;
use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use linked_list_allocator::LockedHeap;
use x86_64::{PhysAddr, VirtAddr};
use x86_64::structures::paging::{FrameAllocator, FrameDeallocator, Mapper, Page, PageSize, PageTable, PageTableFlags, PageTableIndex, PhysFrame, RecursivePageTable, Size4KiB};
use x86_64::structures::paging::mapper::MapToError;

pub const HEAP_START: u64 = 0x_4444_4444_0000;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const INITIAL_HEAP_SIZE: u64 = 1024 * 1024;


/// # Safety
/// Can only be called once.
/// # Panics
/// If recursive index is invalid.
pub unsafe fn init(recursive_index: PageTableIndex, memory_regions: &'static MemoryRegions) -> (RecursivePageTable<'static>, PhysicalMemoryManager) {
    let mut page_table = init_page_table(recursive_index);
    let mut pmm = PhysicalMemoryManager::init_with_heap(memory_regions, &mut page_table);
    (page_table, pmm)
}

fn init_page_table(recursive_index: PageTableIndex) -> RecursivePageTable<'static> {
    let recursive_index: u64 = recursive_index.into();
    let sign = (recursive_index & 0b1000000000) & 0o177777 << 48; // Extend 9th bit of recursive
    let page_table_address = sign | (recursive_index << 39) | (recursive_index << 30) | (recursive_index << 21) | (recursive_index << 12);
    let page_table_pointer = page_table_address as *mut PageTable;
    RecursivePageTable::new(unsafe {&mut *page_table_pointer}).unwrap()
}

pub struct PhysicalMemoryManager {
    free_frames: Vec<PhysFrame>
}

impl PhysicalMemoryManager {
    pub unsafe fn init_with_heap(memory_regions: &'static MemoryRegions, mapper: &mut impl Mapper<Size4KiB>) -> Self {
        Self::init_with_heap_inner(memory_regions, mapper)
    }

    fn init_with_heap_inner(memory_regions: &'static MemoryRegions, mapper: &mut impl Mapper<Size4KiB>) -> PhysicalMemoryManager {
        let mut frame_allocator = unsafe { LinearFrameAllocator::new(memory_regions) };

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
                mapper.map_to(page, frame, flags, &mut frame_allocator).expect("Failed to initialise heap").flush();
            }
        }

        unsafe { ALLOCATOR.lock().init(heap_start.as_mut_ptr(), INITIAL_HEAP_SIZE as usize) };

        // FIXME: This is really slow
        let free_frames: Vec<_> = frame_allocator.available_frames().skip(frame_allocator.next).collect();

        PhysicalMemoryManager { free_frames }
    }
}

unsafe impl FrameAllocator<Size4KiB> for PhysicalMemoryManager {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.free_frames.pop()
    }
}

impl FrameDeallocator<Size4KiB> for PhysicalMemoryManager {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.free_frames.push(frame);
    }
}

struct LinearFrameAllocator {
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