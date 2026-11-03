use anyhow::Result;
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use cobs::CobsDecoder;
use std::{io::Read, os::unix::net::UnixStream};

fn main() -> Result<()> {
    let mut in_buf = [0u8; 1];
    let mut out_buf = [0u8; MAX_MSG_SIZE];

    let mut stream = UnixStream::connect("/tmp/debugcon.sock")?;
    let mut decoder = CobsDecoder::new(&mut out_buf);

    loop {
        stream.read_exact(&mut in_buf)?;
        match decoder.feed(in_buf[0]) {
            Ok(None) => {}
            Ok(Some(decoded)) => {
                if let Ok(event) = postcard::from_bytes::<TraceEvent>(&decoder.dest()[..decoded]) {
                    println!("{:?}", event);
                } else {
                    println!("[WARNING] failed to postcard decode")
                }
            }
            Err(_) => {
                println!("[WARNING] failed to COBS decode")
            }
        }
    }
}
