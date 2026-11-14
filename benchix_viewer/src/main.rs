mod file_source;
mod socket_source;
mod traces;
/// TODO:
/// - [x] File save/load
/// - [x] Start a recording by pressing button
/// - [ ] Search
/// - [x] Trace details panel (right)
/// - [ ] Summary panel (bottom)
use anyhow::Result;
use egui_extras::{Column, TableBuilder};
use rfd::FileDialog;
use std::fmt::Debug;
use std::fs::File;
use std::io::BufWriter;
use std::mem::take;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, CentralPanel, KeyboardShortcut, MenuBar, Modifiers, Panel, ScrollArea, Sense,
};

use crate::file_source::FileSource;
use crate::socket_source::SocketSource;
use crate::traces::{TraceFilters, TraceOwned, Traces};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    eframe::run_native(
        "Benchix Viewer",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )?;

    Ok(())
}

/// Implementations *must* stop sending events when `Drop`ped
trait EventSource: Debug {
    fn collect_events(
        &mut self,
        events: Arc<Mutex<Vec<Arc<TraceOwned>>>>,
        notify: Box<dyn Fn() + Send>,
    );
}

#[derive(Debug)]
enum Command {
    SetSource(Box<dyn EventSource>),
    Save,
}

struct App {
    /// Trace events from kernel
    traces: Traces,
    /// Filter displayed events
    filters: TraceFilters,
    source: Option<Box<dyn EventSource>>,
    /// Commands to perform internal operations in the app
    commands: Vec<Command>,
    summary_open: bool,
    details_open: bool,
    selected_event: Option<Arc<TraceOwned>>,
}

impl App {
    fn new(_cc: &eframe::CreationContext) -> Self {
        Self {
            traces: Traces::new(),
            filters: Default::default(),
            source: None,
            commands: vec![],
            summary_open: true,
            details_open: true,
            selected_event: None,
        }
    }
}

impl App {
    fn set_source(&mut self, ctx: egui::Context, mut source: Box<dyn EventSource>) {
        source.collect_events(
            self.traces.events.clone(),
            Box::new(move || ctx.request_repaint()),
        );
        self.source = Some(source);
    }

    fn save(&self) {
        let events = self.traces.events.lock().unwrap();
        let items: Vec<_> = events.iter().map(|event| event.borrow_event()).collect();

        if let Some(path) = FileDialog::new().save_file() {
            let file = File::create(path).unwrap();
            let writer = BufWriter::new(file);
            let _ = postcard::to_io(&items, writer);
        }
    }

    fn handle_commands(&mut self, ctx: egui::Context) {
        let commands = take(&mut self.commands);
        for command in commands {
            match command {
                Command::SetSource(event_source) => self.set_source(ctx.clone(), event_source),
                Command::Save => self.save(),
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        {
            let events = self.traces.events.lock().unwrap();
            let filtered_events: Vec<_> = events
                .iter()
                .filter(|t| self.filters.filter(t.borrow_event()))
                .collect();

            ui.input_mut(|i| {
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, egui::Key::S)) {
                    self.commands.push(Command::Save);
                }
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, egui::Key::O)) {
                    self.commands.push(Command::SetSource(Box::new(FileSource)));
                }
                if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, egui::Key::L)) {
                    self.commands
                        .push(Command::SetSource(Box::new(SocketSource::new())));
                }
            });

            Panel::top("top_panel").resizable(false).show(ui, |ui| {
                MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ui.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Save (Ctrl+S)").clicked() {
                            self.commands.push(Command::Save);
                        }
                        if ui.button("Open (Ctrl+O)").clicked() {
                            self.commands.push(Command::SetSource(Box::new(FileSource)));
                        }
                        if ui.button("Connect to socket (Ctrl+L)").clicked() {
                            self.commands
                                .push(Command::SetSource(Box::new(SocketSource::new())));
                        }
                    });
                });
            });

            Panel::bottom("bottom_bar").resizable(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} events collected.", events.len()));
                });
            });

            Panel::bottom("summary").resizable(true).show_collapsible(
                ui,
                &mut self.summary_open,
                |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Summary panel");
                        ui.separator();
                    });
                    ui.allocate_space(ui.available_size());
                },
            );

            Panel::right("trace_details")
                .resizable(true)
                .show_collapsible(ui, &mut self.details_open, |ui| {
                    ScrollArea::both().show(ui, |ui| {
                        ui.heading("Trace details panel");
                        ui.separator();
                        match &self.selected_event {
                            Some(ev) => {
                                ui.label(format!("{:x?}", ev.borrow_event()));
                                if ui.button("Clear selection").clicked() {
                                    self.selected_event = None;
                                }
                            }
                            None => {
                                ui.label("No event selected");
                            }
                        }
                    });
                    ui.allocate_space(ui.available_size());
                });

            CentralPanel::default().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Filters");
                    ui.toggle_value(&mut self.filters.syscall_begin, "Syscall begin");
                    ui.toggle_value(&mut self.filters.syscall_end, "Syscall end");
                    ui.toggle_value(&mut self.filters.frame_allocate, "Frame allocate");
                    ui.toggle_value(&mut self.filters.frame_deallocate, "Frame deallocate");
                    ui.toggle_value(
                        &mut self.filters.mark_phys_range,
                        "Mark physical memory range",
                    );
                    ui.toggle_value(
                        &mut self.filters.mark_unusable_range,
                        "Mark unusable frame ranges",
                    );
                    ui.toggle_value(&mut self.filters.heap_allocate, "Heap allocate");
                    ui.toggle_value(&mut self.filters.heap_deallocate, "Heap deallocate");
                    ui.toggle_value(&mut self.filters.context_switch, "Context switch");
                    ui.toggle_value(&mut self.filters.msg, "Message");
                });

                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .stick_to_bottom(true)
                    .column(Column::initial(120.).resizable(true))
                    .column(Column::initial(120.).resizable(true))
                    .column(Column::remainder())
                    .sense(Sense::click())
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.label("Calling address");
                        });
                        header.col(|ui| {
                            ui.label("Timestamp");
                        });
                        header.col(|ui| {
                            ui.label("Trace");
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, filtered_events.len(), |mut row| {
                            let i = row.index();

                            row.set_selected(
                                self.selected_event == Some(Arc::clone(filtered_events[i])),
                            );

                            row.col(|ui| {
                                ui.label(format!(
                                    "{:x}",
                                    filtered_events[i].borrow_event().calling_address
                                ));
                            });
                            row.col(|ui| {
                                ui.label(format!(
                                    "{}",
                                    filtered_events[i].borrow_event().timestamp
                                ));
                            });

                            row.col(|ui| {
                                ui.add(
                                    egui::Label::new(format!(
                                        "{:x?}",
                                        filtered_events[i].borrow_event().trace_type
                                    ))
                                    .truncate()
                                    .selectable(false),
                                );
                            });

                            if row.response().clicked() {
                                self.selected_event = Some(Arc::clone(filtered_events[i]));
                            }
                        });
                    });
            });
        }

        self.handle_commands(ui.clone());
    }
}
