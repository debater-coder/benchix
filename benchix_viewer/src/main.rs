mod socket;
/// TODO:
/// - [ ] File save/load
/// - [ ] Start a recording by pressing button
/// - [ ] Search
/// - [ ] Trace details panel (bottom)
/// - [ ] Summary panel (right)
use anyhow::Result;
use benchix_tracing::TraceType::{
    FrameAllocate, FrameDeallocate, HeapAllocate, HeapDeallocate, Msg, SyscallBegin, SyscallEnd,
};
use benchix_tracing::{MAX_MSG_SIZE, TraceEvent};
use egui_extras::{Column, TableBuilder};
use ouroboros::self_referencing;
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui::{self, CentralPanel, MenuBar, Panel};

use crate::socket::collect_events;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    eframe::run_native(
        "Benchix Viewer",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )?;

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TraceFilters {
    syscall_begin: bool,
    syscall_end: bool,
    frame_allocate: bool,
    frame_deallocate: bool,
    heap_allocate: bool,
    heap_deallocate: bool,
    msg: bool,
}

impl Default for TraceFilters {
    fn default() -> Self {
        Self {
            syscall_begin: true,
            syscall_end: true,
            frame_allocate: true,
            frame_deallocate: true,
            heap_allocate: true,
            heap_deallocate: true,
            msg: true,
        }
    }
}

impl TraceFilters {
    fn filter(&self, event: &TraceEvent) -> bool {
        match event.trace_type {
            SyscallBegin { syscall: _ } if self.syscall_begin => true,
            SyscallEnd { syscall: _ } if self.syscall_end => true,
            FrameAllocate(_) if self.frame_allocate => true,
            FrameDeallocate(_) if self.frame_deallocate => true,
            HeapAllocate { base: _, len: _ } if self.heap_allocate => true,
            HeapDeallocate { base: _, len: _ } if self.heap_deallocate => true,
            Msg(_) if self.msg => true,
            _ => false,
        }
    }
}

#[self_referencing]
struct TraceOwned {
    buf: [u8; MAX_MSG_SIZE],
    #[borrows(buf)]
    #[covariant]
    event: TraceEvent<'this>,
}

struct App {
    events: Arc<Mutex<Vec<TraceOwned>>>,
    filters: TraceFilters,
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
            filters: Default::default(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = self.events.lock().unwrap();
        let filtered_events: Vec<_> = events
            .iter()
            .filter(|t| self.filters.filter(t.borrow_event()))
            .collect();

        Panel::top("top_panel").resizable(false).show(ui, |ui| {
            MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Save").clicked() {
                        ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Load").clicked() {
                        ui.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Connect to socket").clicked() {
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
            ui.horizontal(|ui| {
                ui.label("Filters");
                ui.toggle_value(&mut self.filters.syscall_begin, "Syscall begin");
                ui.toggle_value(&mut self.filters.syscall_end, "Syscall end");
                ui.toggle_value(&mut self.filters.frame_allocate, "Frame allocate");
                ui.toggle_value(&mut self.filters.frame_deallocate, "Frame deallocate");
                ui.toggle_value(&mut self.filters.heap_allocate, "Heap allocate");
                ui.toggle_value(&mut self.filters.heap_deallocate, "Heap deallocate");
                ui.toggle_value(&mut self.filters.msg, "Message");
            });

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .stick_to_bottom(true)
                .column(Column::initial(100.).resizable(true))
                .column(Column::remainder())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.label("Calling address");
                    });
                    header.col(|ui| {
                        ui.label("Trace");
                    });
                })
                .body(|body| {
                    body.rows(20.0, filtered_events.len(), |mut row| {
                        let i = row.index();

                        row.col(|ui| {
                            ui.label(format!(
                                "{:x}",
                                filtered_events[i].borrow_event().calling_address
                            ));
                        });

                        row.col(|ui| {
                            ui.label(format!(
                                "{:x?}",
                                filtered_events[i].borrow_event().trace_type
                            ));
                        });
                    });
                });
        });
    }
}
