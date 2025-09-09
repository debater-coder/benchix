use alloc::{vec, vec::Vec};
use vfs::{Filesystem, FilesystemError};

use crate::VFS;

pub mod devfs;
pub mod ramdisk;
pub mod vfs;

/// Convenience function to read the entirety of a file
pub fn read(path: &str) -> Result<Vec<u8>, FilesystemError> {
    let vfs = VFS.get().unwrap();
    let inode = vfs.traverse_fs(vfs.root.clone(), path)?;
    let mut buffer = vec![0; inode.size];

    vfs.open(inode.clone())?;
    vfs.read(inode.clone(), 0, buffer.as_mut_slice())?;
    vfs.close(inode.clone())?;

    Ok(buffer)
}
