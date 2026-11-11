use std::{
    io::Read,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use cobs::CobsDecoder;
use log::warn;

use crate::{EventSource, TraceOwned, TraceOwnedTryBuilder};

fn collect_events(
    events: Arc<Mutex<Vec<TraceOwned>>>,
    notify: Box<dyn Fn() + Send>,
    running: Arc<AtomicBool>,
) {
    events.lock().unwrap().clear();
    let mut in_buf = [0u8; 1];
    let mut out_buf = [0u8; MAX_MSG_SIZE];

    let mut stream = UnixStream::connect("/tmp/debugcon.sock").unwrap();
    let mut decoder = CobsDecoder::new(&mut out_buf);

    while running.load(Ordering::Relaxed) {
        stream.read_exact(&mut in_buf).unwrap();
        match decoder.feed(in_buf[0]) {
            Ok(None) => {}
            Ok(Some(decoded)) => {
                let mut buf = [0u8; MAX_MSG_SIZE];
                buf[..decoded].copy_from_slice(&decoder.dest()[..decoded]);

                let event_owned = TraceOwnedTryBuilder {
                    buf,
                    event_builder: |buf| postcard::from_bytes::<TraceEvent>(buf),
                }
                .try_build();

                if let Ok(event) = event_owned {
                    let mut lock = events.lock().unwrap();

                    lock.push(event);
                    notify();
                } else {
                    warn!("Postcard decode failed")
                }
            }
            Err(_) => {
                warn!("COBS decode failed")
            }
        }
    }
}

#[derive(Debug)]
pub struct SocketSource {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SocketSource {
    pub fn new() -> Self {
        SocketSource {
            running: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }
}
impl EventSource for SocketSource {
    fn collect_events(
        &mut self,
        events: Arc<Mutex<Vec<TraceOwned>>>,
        notify: Box<dyn Fn() + Send>,
    ) {
        let running = Arc::clone(&self.running);
        self.handle = Some(thread::spawn(move || {
            collect_events(events, notify, running)
        }));
    }
}

impl Drop for SocketSource {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
