use core::ptr::NonNull;

use acpi::{AcpiHandler, PhysicalMapping};
use x86_64::VirtAddr;

#[derive(Clone, Copy)]
pub struct Handler {
    pub phys_offset: VirtAddr,
}

impl AcpiHandler for Handler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        // Doesn't actually map anything, just uses the physical offset
        PhysicalMapping::new(
            physical_address,
            NonNull::new((self.phys_offset + physical_address as u64).as_mut_ptr()).unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(_region: &acpi::PhysicalMapping<Self, T>) {}
}
