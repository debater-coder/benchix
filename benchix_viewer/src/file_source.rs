use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use log::error;
use rfd::FileDialog;

use crate::{EventSource, TraceOwnedBuilder};

fn collect_events() -> Result<Vec<crate::TraceOwned>> {
    let path = FileDialog::new()
        .pick_file()
        .ok_or(anyhow!("Failed to pick file"))?;
    let buf = Box::leak(std::fs::read(path)?.into_boxed_slice());
    let events: Vec<TraceEvent<'static>> = postcard::from_bytes(buf)?;

    Ok(events
        .iter()
        .map(|event| {
            TraceOwnedBuilder {
                buf: [0u8; MAX_MSG_SIZE],
                event_builder: |_buf| *event,
            }
            .build()
        })
        .collect())
}

#[derive(Debug)]
pub struct FileSource;

impl EventSource for FileSource {
    fn collect_events(
        &mut self,
        events: Arc<Mutex<Vec<crate::TraceOwned>>>,
        notify: Box<dyn Fn() + Send>,
    ) {
        match collect_events() {
            Ok(collected) => {
                *events.lock().unwrap() = collected;
            }
            Err(e) => error!("Failed to load file {:?}", e),
        }
        notify();
    }
}
