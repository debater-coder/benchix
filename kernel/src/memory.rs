use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use linked_list_allocator::LockedHeap;
use x86_64::{PhysAddr, VirtAddr};
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTable, PageTableFlags, PageTableIndex, PhysFrame, RecursivePageTable, Size4KiB};
use x86_64::structures::paging::mapper::MapToError;

pub const HEAP_START: u64 = 0x_4444_4444_0000;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const HEAP_SIZE: u64 = 100 * 1024; // 100 KiB


/// # Safety
/// Can only be called once.
/// # Panics
/// If recursive index is invalid.
pub unsafe fn init(recursive_index: PageTableIndex, memory_regions: &'static MemoryRegions) -> (RecursivePageTable<'static>, BootInfoFrameAllocator) {
    let mut page_table = init_page_table(recursive_index);
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::new(memory_regions) };
    init_heap(&mut frame_allocator, &mut page_table).expect("Failed to initialise heap");

    (page_table, frame_allocator)
}

fn init_page_table(recursive_index: PageTableIndex) -> RecursivePageTable<'static> {
    let recursive_index: u64 = recursive_index.into();
    let sign = (recursive_index & 0b1000000000) & 0o177777 << 48; // Extend 9th bit of recursive
    let page_table_address = sign | (recursive_index << 39) | (recursive_index << 30) | (recursive_index << 21) | (recursive_index << 12);
    let page_table_pointer = page_table_address as *mut PageTable;
    RecursivePageTable::new(unsafe {&mut *page_table_pointer}).unwrap()
}

fn init_heap(
    frame_allocator: &mut BootInfoFrameAllocator,
    mapper: &mut RecursivePageTable<'static>,
) -> Result<(), MapToError<Size4KiB>> {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE - 1u64;
    let page_range = Page::range_inclusive(
        Page::containing_address(heap_start),
        Page::containing_address(heap_end),
    );

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(heap_start.as_mut_ptr(), HEAP_SIZE as usize);
    }

    Ok(())
}

pub struct BootInfoFrameAllocator {
    next: usize,
    memory_regions: &'static MemoryRegions,
}

impl BootInfoFrameAllocator {
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
    pub unsafe fn new(memory_regions: &'static MemoryRegions) -> Self {
        BootInfoFrameAllocator {
            next: 0,
            memory_regions,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = self.available_frames().nth(self.next);
        self.next += 1;
        frame
    }
}