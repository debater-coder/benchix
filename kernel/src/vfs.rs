use alloc::{boxed::Box, collections::btree_map::BTreeMap, sync::Arc, vec::Vec};

pub enum FilesystemError {
    UnknownDevice,
    WrongType, // i.e file not directory or other way
    NotFound,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FileType {
    File,
    Directory,
    Device, // Just block devices for now, we don't have a good distinction between buffered/unbuffered devices
    Mountpoint,
}

/// # VFS in-memory inode
/// Note that this doesn't contain a list of block addresses, as individual file systems are responsible for maintaining their own inode cache
/// Once a filesystem is registered on the VFS, these inodes contain all the info needed to stand in for the real on-disk inodes
pub struct Inode {
    pub dev: u32,   // File system that the file belongs to
    pub inode: u32, // inode number
    pub file_type: FileType,
    pub size: usize,             // 0 for devices
    pub major: Option<u32>,      // Device driver
    pub minor: Option<u32>,      // Specific device that belongs to driver
    pub ptr: Option<(u32, u32)>, // (dev, inode) for mountpoints (if a file is a mountpoint it has size 0 bytes and is jsut basically this tuple)
}

/// # Why is `name` an Arc<str>?
/// `name` is Arc<str> because it is a string reference to a block in the inode cache.
/// This Arc is created by borrowing the Arc that wraps blocks. When this Arc is freed, it will decrement the block Arc.
pub struct DirectoryEntry {
    pub name: Arc<str>,
    pub inode: u32,
    pub dev: u32,
}

pub trait Filesystem {
    fn open(&self, inode: Arc<Inode>) -> Result<(), FilesystemError>; // Usually increments a reference counter or starts caching inodes -- doesn't lock the file
    fn close(&self, inode: Arc<Inode>) -> Result<(), FilesystemError>; // Decrements the reference counter, or removes from cache
    fn read(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<(), FilesystemError>; // A locking operation
    fn write(&self, inode: Arc<Inode>, offset: u64, buffer: &[u8]) -> Result<(), FilesystemError>; // A locking operation
    fn readdir(&self, inode: Arc<Inode>) -> Result<Vec<DirectoryEntry>, FilesystemError>; // Get directory enteries
    fn inode(&self, dev: u32, inode: u32) -> Result<Arc<Inode>, FilesystemError>; // An inode lookup
}

/// Finds the inode relative to a root inode (assuming its a directory)
pub fn traverse_fs(
    filesystem: &impl Filesystem,
    root: Arc<Inode>,
    path: &str,
) -> Result<Arc<Inode>, FilesystemError> {
    path.split("/").fold(Ok(root), |inode, segment| {
        let (dev, ino) = filesystem
            .readdir(inode?)?
            .iter()
            .find(|dirent| *dirent.name == *segment)
            .ok_or(FilesystemError::NotFound)
            .map(|dirent| (dirent.dev, dirent.inode))?;

        filesystem.inode(dev, ino)
    })
}

pub struct VirtualFileSystem {
    filesystems: BTreeMap<u32, Box<dyn Filesystem>>,
    dirents: Vec<DirectoryEntry>,
    pub root: Inode,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        VirtualFileSystem {
            filesystems: BTreeMap::new(),
            dirents: Vec::new(),
            root: Inode {
                dev: 0,
                inode: 0,
                file_type: FileType::Directory,
                size: 0,
                major: None,
                minor: None,
                ptr: None,
            },
        }
    }

    pub fn mount(&mut self, dev: u32, filesystem: Box<dyn Filesystem>, path: &str) {
        // Add filesystem with correct device
        self.filesystems.insert(dev, filesystem);

        // Mount to directory
        // TODO: Traversing directory logic
    }
}

impl Filesystem for VirtualFileSystem {
    fn open(&self, inode: Arc<Inode>) -> Result<(), FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .open(inode),
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn close(&self, inode: Arc<Inode>) -> Result<(), FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .close(inode),
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn read(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<(), FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .read(inode, offset, buffer),
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn write(&self, inode: Arc<Inode>, offset: u64, buffer: &[u8]) -> Result<(), FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .write(inode, offset, buffer),
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn readdir(&self, inode: Arc<Inode>) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        match inode.file_type {
            FileType::Directory => Ok(self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .readdir(inode)?),
            FileType::Mountpoint => {
                let (dev, inode) = inode.ptr.expect("mountpoint should have ptr");
                let filesystem = self
                    .filesystems
                    .get(&dev)
                    .ok_or(FilesystemError::UnknownDevice)?;
                filesystem.readdir(filesystem.inode(dev, inode)?)
            }
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn inode(&self, dev: u32, inode: u32) -> Result<Arc<Inode>, FilesystemError> {
        Ok(self
            .filesystems
            .get(&dev)
            .ok_or(FilesystemError::UnknownDevice)?
            .inode(dev, inode)?)
    }
}
