#[derive(Debug)]
pub enum LoadingError {
    InvalidHeader,
}

#[derive(Debug)]
#[repr(C)]
pub(super) struct ProgramHeaderEntry {
    pub(super) segment_type: u64, // contains both p_type and p_flags
    pub(super) offset: u64,
    pub(super) virtual_address: u64,
    pub(super) unused: u64,
    pub(super) image_size: u64,
    pub(super) mem_size: u64,
    pub(super) align: u64,
}
