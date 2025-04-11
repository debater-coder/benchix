use alloc::{borrow::ToOwned, sync::Arc, vec};
use spin::Mutex;

use crate::console::Console;

use super::vfs::{DirectoryEntry, FileType, Filesystem, FilesystemError, Inode};

pub struct Devfs {
    console: Mutex<Console>,
    root: Arc<Inode>,
    console_inode: Arc<Inode>,
}

impl Devfs {
    pub fn new(console: Console, dev: u32) -> Self {
        Devfs {
            console: Mutex::new(console),
            root: Arc::new(Inode {
                dev,
                inode: 0,
                file_type: FileType::Directory,
                size: 0,
                major: None,
                minor: None,
            }),
            console_inode: Arc::new(Inode {
                dev,
                inode: 1,
                file_type: FileType::Device,
                size: 0,
                major: Some(1),
                minor: Some(1),
            }),
        }
    }
}

impl Filesystem for Devfs {
    fn open(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
    ) -> Result<(), super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            Ok(())
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn close(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
    ) -> Result<(), super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            Ok(())
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn read(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            Ok(self.console.lock().read(buffer))
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn write(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
        offset: u64,
        buffer: &[u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            Ok(self.console.lock().write(buffer))
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn readdir(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
    ) -> Result<alloc::vec::Vec<super::vfs::DirectoryEntry>, super::vfs::FilesystemError> {
        if inode.dev == self.root.dev && inode.inode == self.root.inode {
            Ok(vec![DirectoryEntry {
                name: "console".to_owned(),
                inode: 1,
                dev: self.root.dev,
            }])
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn inode(
        &self,
        dev: u32,
        inode: u32,
    ) -> Result<alloc::sync::Arc<super::vfs::Inode>, super::vfs::FilesystemError> {
        if dev != self.root.dev {
            return Err(FilesystemError::NotFound);
        }

        match inode {
            0 => Ok(Arc::clone(&self.root)),
            1 => Ok(Arc::clone(&self.console_inode)),
            _ => Err(FilesystemError::NotFound),
        }
    }
}
