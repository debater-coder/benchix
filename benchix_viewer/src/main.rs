use anyhow::Result;
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use cobs::CobsDecoder;
use egui_extras::{Column, TableBuilder};
use log::warn;
use std::sync::{Arc, Mutex};
use std::thread;
use std::{io::Read, os::unix::net::UnixStream};

use eframe::egui::{self, CentralPanel, MenuBar, Panel};

fn collect_events<F>(events: Arc<Mutex<Vec<TraceEvent<'static>>>>, notify: F) -> Result<()>
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
                if let Ok(event) = postcard::from_bytes::<TraceEvent>(Box::leak(Box::from(
                    &decoder.dest()[..decoded],
                ))) {
                    let mut lock = events.lock().unwrap();
                    lock.push(event.clone());
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

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    eframe::run_native(
        "Benchix Viewer",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )?;

    Ok(())
}

struct App {
    events: Arc<Mutex<Vec<TraceEvent<'static>>>>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let events = Arc::new(Mutex::new(vec![]));
        let events_for_thread = Arc::clone(&events);
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            collect_events(events_for_thread, move || {
                ctx.request_repaint();
            })
        });
        Self {
            events: Arc::clone(&events),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let events = self.events.lock().unwrap();

        Panel::top("top_panel").resizable(false).show(ui, |ui| {
            MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

        Panel::bottom("bottom_bar").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} events collected.", events.len()));
            });
        });

        CentralPanel::default().show(ui, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .stick_to_bottom(true)
                .column(Column::auto().resizable(true))
                .column(Column::remainder())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.heading("Calling address");
                    });
                    header.col(|ui| {
                        ui.heading("Trace");
                    });
                })
                .body(|body| {
                    body.rows(20.0, events.len(), |mut row| {
                        let i = row.index();

                        row.col(|ui| {
                            ui.label(format!("{:x}", events[i].calling_address));
                        });

                        row.col(|ui| {
                            ui.label(format!("{:?}", events[i].trace_type));
                        });
                    });
                });
        });
    }
}
