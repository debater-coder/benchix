use alloc::{borrow::ToOwned, string::ToString, sync::Arc, vec, vec::Vec};
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use pc_keyboard::{
    DecodedKey, EventDecoder, HandleControl, ScancodeSet, ScancodeSet1, layouts::Us104Key,
};
use spin::Mutex;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    CPUS,
    console::Console,
    scheduler::{self, Thread},
};

pub static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();

/// DANGER LOCK: DISABLE INTERRUPTS BEFORE USE!!!
pub static WAITING_THREAD: Mutex<Option<Arc<Mutex<Thread>>>> = Mutex::new(None);

use super::vfs::{DirectoryEntry, FileType, Filesystem, FilesystemError, Inode};

pub struct Devfs {
    console: Mutex<Console>,
    root: Arc<Inode>,
    console_inode: Arc<Inode>,
    pending_input: Mutex<Vec<u8>>,
    scancode_set: Mutex<ScancodeSet1>,
    event_decoder: Mutex<EventDecoder<Us104Key>>,
}

impl Devfs {
    pub fn init(console: Console, dev: u32) -> Self {
        SCANCODE_QUEUE
            .try_init_once(|| ArrayQueue::new(100))
            .expect("Devfs::init() can only be called once.");
        Devfs {
            console: Mutex::new(console),
            root: Arc::new(Inode {
                dev,
                inode: 0,
                file_type: FileType::Directory,
                size: 0,
                major: None,
                minor: None,
                inner: None,
            }),
            console_inode: Arc::new(Inode {
                dev,
                inode: 1,
                file_type: FileType::Device,
                size: 0,
                major: Some(1),
                minor: Some(1),
                inner: None,
            }),
            pending_input: Mutex::new(Vec::new()),
            scancode_set: Mutex::new(ScancodeSet1::new()),
            event_decoder: Mutex::new(EventDecoder::new(
                Us104Key,
                HandleControl::MapLettersToUnicode,
            )),
        }
    }

    pub fn push_scancode(scancode: u8) {
        if let Some(queue) = SCANCODE_QUEUE.get() {
            queue.force_push(scancode); // So that older scancodes are discarded
        }

        // Wake up sleeping thread
        if let Some(thread) = without_interrupts(|| WAITING_THREAD.lock().clone()) {
            scheduler::enqueue(thread);
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
        _offset: u64,
        buffer: &mut [u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            while {
                while self.pending_input.lock().len() < buffer.len()
                    && let Some(scancode) = SCANCODE_QUEUE.get().unwrap().pop()
                {
                    let key_event = self.scancode_set.lock().advance_state(scancode).unwrap();

                    let decoded_key = if let Some(event) = key_event {
                        self.event_decoder.lock().process_keyevent(event)
                    } else {
                        None
                    };

                    match decoded_key {
                        Some(DecodedKey::Unicode(key)) => {
                            let key = key.to_string();
                            let key = key.as_str().as_bytes();
                            self.console.lock().write(key);
                            self.pending_input.lock().append(&mut Vec::from(key));
                        }
                        _ => (),
                    }
                }
                let input = self.pending_input.lock();
                let last = input.last();
                input.len() < buffer.len() && last != Some(&b'\n') && last != Some(&4)
            } {
                without_interrupts(|| {
                    *WAITING_THREAD.lock() = Some(
                        CPUS.get()
                            .unwrap()
                            .get_cpu()
                            .current_thread
                            .as_ref()
                            .unwrap()
                            .clone(),
                    )
                });

                scheduler::yield_execution();
            }

            let mut lock = self.pending_input.lock();

            // Replace EOF with null terminator
            if lock.last() == Some(&4) {
                *lock.last_mut().unwrap() = 0;
            }

            let result = &lock[..buffer.len().min(lock.len())];
            buffer[..result.len()].copy_from_slice(result);
            let len = result.len();

            *lock = lock[len..].to_owned();

            Ok(len)
        } else {
            Err(FilesystemError::NotFound)
        }
    }

    fn write(
        &self,
        inode: alloc::sync::Arc<super::vfs::Inode>,
        _offset: u64,
        buffer: &[u8],
    ) -> Result<usize, super::vfs::FilesystemError> {
        if let (Some(1), Some(1)) = (inode.major, inode.minor) {
            debug_println!("{}", str::from_utf8(buffer).unwrap_or("0"));
            without_interrupts(|| Ok(self.console.lock().write(buffer)))
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
