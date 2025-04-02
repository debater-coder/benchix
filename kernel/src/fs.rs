use alloc::{boxed::Box, string::String, vec::Vec};

enum FilesystemError {
    UnknownDevice,
    WrongType, // i.e file not directory or other way
}

#[derive(Debug, Eq, PartialEq)]
enum FileType {
    File,
    Directory,
    Device, // Just block devices for now, we don't have a good distinction between buffered/unbuffered devices
    Mountpoint,
}

/// # VFS in-memory inode
/// Note that this doesn't contain a list of block addresses, as individual file systems are responsible for maintaining their own inode cache
/// Once a filesystem is registered on the VFS, these inodes contain all the info needed to stand in for the real on-disk inodes
struct Inode {
    pub dev: u8,    // File system that the file belongs to
    pub inode: u32, // inode number
    pub file_type: FileType,
    pub size: usize,        // 0 for devices
    pub major: Option<u32>, // Device driver
    pub minor: Option<u32>, // Specific device that belongs to driver
    pub ptr: (u8, u32), // (dev, inode) for mountpoints (if a file is a mountpoint it has size 0 bytes and is jsut basically this tuple)
}

struct DirectoryEntry {
    name: String, // The use of an owned string is deliberate; VFS-dirents may outlive the cached blocks that physically hold the filenames
    inode: u32,
}

trait Filesystem {
    fn open(&self, inode: &Inode) -> Result<(), FilesystemError>; // Usually increments a reference counter or starts caching inodes -- doesn't lock the file
    fn close(&self, inode: &Inode) -> Result<(), FilesystemError>; // Decrements the reference counter, or removes from cache
    fn read(&self, inode: &Inode, offset: u64, buffer: &mut [u8]) -> Result<(), FilesystemError>; // A locking operation
    fn readdir(&self, inode: &Inode) -> Result<Vec<DirectoryEntry>, FilesystemError>;
    fn inode(&self, dev: u8, inode: u32) -> Result<&Inode, FilesystemError>; // An inode lookup
}

pub struct VirtualFileSystem {
    pub filesystems: [Option<Box<dyn Filesystem>>; 256], // Index of filesystem is devid
}

impl Filesystem for VirtualFileSystem {
    fn open(&self, inode: &Inode) -> Result<(), FilesystemError> {
        self.filesystems[inode.dev as usize]
            .as_ref()
            .ok_or(FilesystemError::UnknownDevice)?
            .open(inode);
        Ok(())
    }

    fn close(&self, inode: &Inode) -> Result<(), FilesystemError> {
        self.filesystems[inode.dev as usize]
            .as_ref()
            .ok_or(FilesystemError::UnknownDevice)?
            .close(inode);
        Ok(())
    }

    fn read(&self, inode: &Inode, offset: u64, buffer: &mut [u8]) -> Result<(), FilesystemError> {
        self.filesystems[inode.dev as usize]
            .as_ref()
            .ok_or(FilesystemError::UnknownDevice)?
            .read(inode, offset, buffer);
        Ok(())
    }

    fn readdir(&self, inode: &Inode) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        match inode.file_type {
            FileType::Directory => Ok(self.filesystems[inode.dev as usize]
                .as_ref()
                .ok_or(FilesystemError::UnknownDevice)?
                .readdir(inode)?),
            FileType::Mountpoint => {
                let filesystem = self.filesystems[inode.ptr.0 as usize]
                    .as_ref()
                    .ok_or(FilesystemError::UnknownDevice)?;
                Ok(filesystem.readdir(filesystem.inode(inode.ptr.0, inode.ptr.1)?)?)
            }
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn inode(&self, dev: u8, inode: u32) -> Result<&Inode, FilesystemError> {
        Ok(self.filesystems[dev as usize]
            .as_ref()
            .ok_or(FilesystemError::UnknownDevice)?
            .inode(dev, inode)?)
    }
}
