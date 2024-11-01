use bootloader_api::info::MemoryRegions;
use linked_list_allocator::LockedHeap;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::OffsetPageTable;
use x86_64::VirtAddr;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();



/// # Safety
/// Can only be called once. Physical offset must be correct
pub unsafe fn init(physical_offset: u64, memory_regions: &'static MemoryRegions) -> (OffsetPageTable<'static>,) {
    let mapper = init_page_table(physical_offset);

    (mapper, )
}

fn init_page_table(physical_offset: u64) -> OffsetPageTable<'static> {
    let physical_offset = VirtAddr::new(physical_offset);

    let (l4_page_table_phys, _) = Cr3::read();

    let l4_page_table_addr = physical_offset + l4_page_table_phys.start_address().as_u64();
    let l4_page_table = l4_page_table_addr.as_mut_ptr();

    unsafe { OffsetPageTable::new(&mut *l4_page_table, physical_offset) }
}
