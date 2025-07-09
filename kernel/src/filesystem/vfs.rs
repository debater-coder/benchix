use core::any::Any;

use alloc::{
    borrow::ToOwned, boxed::Box, collections::btree_map::BTreeMap, string::String, sync::Arc,
    vec::Vec,
};

#[derive(Debug)]
pub enum FilesystemError {
    UnknownDevice,
    WrongType, // i.e file not directory or other way
    NotFound,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum FileType {
    File,
    Directory,
    Device, // Just block devices for now, we don't have a good distinction between buffered/unbuffered devices
}

/// # VFS in-memory inode
/// Note that this doesn't contain a list of block addresses, as individual file systems are responsible for maintaining their own inode cache
/// Once a filesystem is registered on the VFS, these inodes contain all the info needed to stand in for the real on-disk inodes
#[derive(Debug)]
pub struct Inode {
    pub dev: u32,   // File system that the file belongs to
    pub inode: u32, // inode number
    pub file_type: FileType,
    pub size: usize,        // 0 for devices
    pub major: Option<u32>, // Device driver
    pub minor: Option<u32>, // Specific device that belongs to driver
    pub inner: Option<Box<dyn Any + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    pub name: String,
    pub inode: u32,
    pub dev: u32,
}

pub trait Filesystem: Send + Sync {
    fn open(&self, inode: Arc<Inode>) -> Result<(), FilesystemError>; // Usually increments a reference counter or starts caching inodes -- doesn't lock the file
    fn close(&self, inode: Arc<Inode>) -> Result<(), FilesystemError>; // Decrements the reference counter, or removes from cache
    fn read(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<usize, FilesystemError>; // A locking operation
    fn write(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &[u8],
    ) -> Result<usize, FilesystemError>; // A locking operation
    fn readdir(&self, inode: Arc<Inode>) -> Result<Vec<DirectoryEntry>, FilesystemError>; // Get directory enteries
    fn inode(&self, dev: u32, inode: u32) -> Result<Arc<Inode>, FilesystemError>; // An inode lookup
    fn traverse_fs(&self, root: Arc<Inode>, path: &str) -> Result<Arc<Inode>, FilesystemError> {
        path.split("/").fold(Ok(root), |inode, segment| {
            if segment == "" {
                return inode;
            }
            let (dev, ino) = self
                .readdir(inode?)?
                .iter()
                .find(|dirent| *dirent.name == *segment)
                .ok_or(FilesystemError::NotFound)
                .map(|dirent| (dirent.dev, dirent.inode))?;

            self.inode(dev, ino)
        })
    }
}

pub struct VirtualFileSystem {
    filesystems: BTreeMap<u32, Box<dyn Filesystem>>,
    dirents: Vec<DirectoryEntry>,
    pub root: Arc<Inode>,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        VirtualFileSystem {
            filesystems: BTreeMap::new(),
            dirents: Vec::new(),
            root: Arc::new(Inode {
                dev: 0,
                inode: 0,
                file_type: FileType::Directory,
                size: 0,
                major: None,
                minor: None,
                inner: None,
            }),
        }
    }

    /// Only allows root mounting
    pub fn mount(
        &mut self,
        dev: u32,
        filesystem: Box<dyn Filesystem>,
        name: &str,
        root_inode: u32,
    ) -> Result<(), FilesystemError> {
        // Add filesystem with correct device
        self.filesystems.insert(dev, filesystem);
        self.dirents.push(DirectoryEntry {
            name: name.to_owned(),
            inode: root_inode,
            dev,
        });

        Ok(())
    }
}

impl Filesystem for VirtualFileSystem {
    fn open(&self, inode: Arc<Inode>) -> Result<(), FilesystemError> {
        if inode.dev == 0 {
            return Ok(()); // Root inode has no implementation
        }

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
        if inode.dev == 0 {
            return Ok(());
        }

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
    ) -> Result<usize, FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .read(inode, offset, buffer),
            _ => Err(FilesystemError::WrongType), // Root inode
        }
    }

    fn write(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &[u8],
    ) -> Result<usize, FilesystemError> {
        match inode.file_type {
            FileType::Device | FileType::File => self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .write(inode, offset, buffer),
            _ => Err(FilesystemError::WrongType), // Root inode
        }
    }

    fn readdir(&self, inode: Arc<Inode>) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        if inode.dev == 0 && inode.inode == 0 {
            return Ok(self.dirents.clone());
        }

        match inode.file_type {
            FileType::Directory => Ok(self
                .filesystems
                .get(&inode.dev)
                .ok_or(FilesystemError::UnknownDevice)?
                .readdir(inode)?),
            _ => Err(FilesystemError::WrongType),
        }
    }

    fn inode(&self, dev: u32, inode: u32) -> Result<Arc<Inode>, FilesystemError> {
        if dev == 0 && inode == 0 {
            return Ok(Arc::clone(&self.root));
        }
        Ok(self
            .filesystems
            .get(&dev)
            .ok_or(FilesystemError::UnknownDevice)?
            .inode(dev, inode)?)
    }
}
