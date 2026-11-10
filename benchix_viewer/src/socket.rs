use std::{
    io::Read,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use cobs::CobsDecoder;
use log::warn;

use crate::{TraceOwned, TraceOwnedTryBuilder};

pub fn collect_events<F>(events: Arc<Mutex<Vec<TraceOwned>>>, notify: F) -> Result<()>
where
    F: Fn(),
{
    let mut in_buf = [0u8; 1];
    let mut out_buf = [0u8; MAX_MSG_SIZE];

    let mut stream = UnixStream::connect("/tmp/debugcon.sock")?;
    let mut decoder = CobsDecoder::new(&mut out_buf);

    loop {
        stream.read_exact(&mut in_buf)?;
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
