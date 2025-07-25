use alloc::{boxed::Box, collections::btree_map::BTreeMap, string::ToString, sync::Arc, vec::Vec};

use super::vfs::{DirectoryEntry, FileType, Filesystem, FilesystemError, Inode};

// Only supports one level of directories
pub struct Initrd {
    pub dev: u32,
    inodes: BTreeMap<u32, Arc<Inode>>,
}

impl Initrd {
    pub fn from_files(dev: u32, files: Vec<(&str, &'static [u8])>) -> Self {
        let mut map: BTreeMap<u32, Arc<Inode>> = BTreeMap::new();

        map.insert(
            0,
            Arc::new(Inode {
                dev,
                inode: 0,
                file_type: FileType::Directory,
                size: 0,
                major: None,
                minor: None,
                inner: Some(Box::new(
                    files
                        .iter()
                        .enumerate()
                        .map(|(index, (filename, _))| DirectoryEntry {
                            dev,
                            inode: index as u32 + 1,
                            name: filename.to_string(),
                        })
                        .collect::<Vec<_>>(),
                )),
            }),
        );

        for (index, (_, contents)) in files.iter().enumerate() {
            map.insert(
                index as u32 + 1,
                Arc::new(Inode {
                    dev,
                    inode: index as u32 + 1,
                    file_type: FileType::File,
                    size: contents.len(),
                    major: None,
                    minor: None,
                    inner: Some(Box::new(*contents)),
                }),
            );
        }

        Initrd { dev, inodes: map }
    }
}

impl Filesystem for Initrd {
    fn open(&self, _inode: Arc<Inode>) -> Result<(), super::vfs::FilesystemError> {
        Ok(())
    }

    fn close(&self, _inode: Arc<Inode>) -> Result<(), super::vfs::FilesystemError> {
        Ok(())
    }

    fn read(
        &self,
        inode: Arc<Inode>,
        offset: u64,
        buffer: &mut [u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        if inode.file_type != FileType::File || inode.dev != self.dev {
            return Err(FilesystemError::WrongType);
        }

        let offset = offset as usize;
        let contents = inode
            .inner
            .as_ref()
            .ok_or(FilesystemError::WrongType)?
            .downcast_ref::<&'static [u8]>()
            .ok_or(FilesystemError::WrongType)?;
        let contents = &contents[offset..(offset + buffer.len()).min(inode.size)];
        buffer[..contents.len()].copy_from_slice(contents);
        Ok(contents.len())
    }

    fn write(
        &self,
        _inode: Arc<Inode>,
        _offset: u64,
        _buffer: &[u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        Err(FilesystemError::WrongType)
    }

    fn readdir(
        &self,
        inode: Arc<Inode>,
    ) -> Result<Vec<DirectoryEntry>, super::vfs::FilesystemError> {
        if inode.file_type != FileType::Directory || inode.dev != self.dev {
            return Err(FilesystemError::WrongType);
        }

        Ok(inode
            .inner
            .as_ref()
            .ok_or(FilesystemError::WrongType)?
            .downcast_ref::<Vec<DirectoryEntry>>()
            .ok_or(FilesystemError::WrongType)?
            .clone())
    }

    fn inode(&self, dev: u32, inode: u32) -> Result<Arc<Inode>, super::vfs::FilesystemError> {
        if dev != self.dev {
            return Err(FilesystemError::WrongType);
        }

        Ok(self
            .inodes
            .get(&inode)
            .ok_or(FilesystemError::NotFound)?
            .clone())
    }
}
