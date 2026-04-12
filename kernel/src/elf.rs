use core::{ffi::CStr, slice};

#[derive(Debug)]
pub enum ElfError {
    InvalidHeader,
    SymbolTableNotFound,
    InvalidSection,
    StringTableNotFound,
    FailedToDecodeString,
}

#[derive(Debug)]
#[repr(C)]
pub struct ProgramHeaderEntry {
    pub segment_type: u64, // contains both p_type and p_flags
    pub offset: u64,
    pub virtual_address: u64,
    pub unused: u64,
    pub image_size: u64,
    pub mem_size: u64,
    pub align: u64,
}

#[derive(Debug)]
#[repr(C)]
pub struct SectionHeaderEntry {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

/// https://wiki.osdev.org/ELF_Tutorial#ELF_Sections
enum SectionHeaderEntryTypes {
    Null = 0,     // Null section
    Progbits = 1, // Program information
    Symtab = 2,   // Symbol table
    Strtab = 3,   // String table
    Rela = 4,     // Relocation (with addend)
    Nobits = 8,   // Not present in file
    Rel = 9,      // Relocation (no addend)
}

#[derive(Debug)]
#[repr(C)]
pub struct SymbolTableEntry {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

pub struct Elf<'a> {
    binary: &'a [u8],
    pub executable: bool,
}

impl<'a> Elf<'a> {
    pub fn new(binary: &'a [u8]) -> Result<Self, ElfError> {
        if binary[0x0..0x4] != *b"\x7fELF" // Magic
            || binary[0x4] != 2 // 64-bit
            || binary[0x5] != 1
        // Little endian
        {
            return Err(ElfError::InvalidHeader);
        }

        // Validate ELF
        Ok(Elf {
            binary,
            executable: binary[0x10] == 2,
        })
    }

    pub fn program_headers(&self) -> Result<impl Iterator<Item = &ProgramHeaderEntry>, ElfError> {
        let header_start = u64::from_ne_bytes(self.binary[0x20..0x28].try_into().unwrap()) as usize;
        let header_size = u16::from_ne_bytes(self.binary[0x36..0x38].try_into().unwrap()) as usize;
        let header_num = u16::from_ne_bytes(self.binary[0x38..0x3A].try_into().unwrap()) as usize;

        if header_size < size_of::<ProgramHeaderEntry>() {
            return Err(ElfError::InvalidHeader);
        }

        Ok((0..header_num)
            .map(move |i| header_start + header_size * i)
            .map(|offset| unsafe {
                &*(self.binary[offset..(offset + size_of::<ProgramHeaderEntry>())].as_ptr()
                    as *const ProgramHeaderEntry)
            }))
    }

    pub fn section_headers(&self) -> Result<impl Iterator<Item = &SectionHeaderEntry>, ElfError> {
        let header_start = u64::from_ne_bytes(self.binary[0x28..0x30].try_into().unwrap()) as usize;
        let header_size = u16::from_ne_bytes(self.binary[0x3A..0x3C].try_into().unwrap()) as usize;
        let header_num = u16::from_ne_bytes(self.binary[0x3C..0x3E].try_into().unwrap()) as usize;

        if header_size < size_of::<SectionHeaderEntry>() {
            return Err(ElfError::InvalidHeader);
        }

        Ok((0..header_num)
            .map(move |i| header_start + header_size * i)
            .map(|offset| unsafe {
                &*(self.binary[offset..(offset + size_of::<SectionHeaderEntry>())].as_ptr()
                    as *const SectionHeaderEntry)
            }))
    }

    pub fn get_symbols(&self) -> Result<(&[SymbolTableEntry], u32), ElfError> {
        let symtab_entry = self
            .section_headers()?
            .find(|hdr| hdr.sh_type == SectionHeaderEntryTypes::Symtab as u32)
            .ok_or(ElfError::SymbolTableNotFound)?;

        if symtab_entry.sh_entsize as usize != size_of::<SymbolTableEntry>() {
            return Err(ElfError::InvalidSection);
        }

        Ok((
            unsafe {
                slice::from_raw_parts(
                    &*(self.binary[symtab_entry.sh_offset as usize
                        ..(symtab_entry.sh_offset + symtab_entry.sh_size) as usize]
                        .as_ptr() as *const SymbolTableEntry),
                    (symtab_entry.sh_size / symtab_entry.sh_entsize) as usize,
                )
            },
            symtab_entry.sh_link,
        ))
    }

    pub fn lookup_strtab(&self, strtab: u32, offset: u32) -> Result<Option<&str>, ElfError> {
        let strtab_entry = self
            .section_headers()?
            .nth(strtab as usize)
            .ok_or(ElfError::StringTableNotFound)?;
        let strtab = &self.binary[strtab_entry.sh_offset as usize
            ..(strtab_entry.sh_offset + strtab_entry.sh_size) as usize];
        if offset == 0 {
            return Ok(None);
        }

        Ok(Some(
            CStr::from_bytes_until_nul(&strtab[offset as usize..])
                .map_err(|_| ElfError::FailedToDecodeString)?
                .to_str()
                .map_err(|_| ElfError::FailedToDecodeString)?,
        ))
    }
}
