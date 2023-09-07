// Copyright 2023 Dimitris Papaioannou <dimtpap@protonmail.com>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published by
// the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.
//
// SPDX-License-Identifier: GPL-3.0-only

use eframe::egui;
use pipewire::types::ObjectType;

#[cfg(feature = "xdg_desktop_portals")]
use ashpd::{desktop::screencast::SourceType, enumflags2::BitFlags};

use crate::backend::{self, Event, RemoteInfo};

use super::{
    common::EditableKVList, globals_store::ObjectData, ContextManager, GlobalsStore,
    MetadataEditor, ObjectCreator, Profiler, WindowedTool,
};

#[derive(Clone, Copy)]
enum View {
    GlobalTracker = 1 << 0,
    Profiler = 1 << 1,
    ProcessViewer = 1 << 2,
}

impl View {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Profiler => "Profiler",
            Self::ProcessViewer => "Process Viewer",
            Self::GlobalTracker => "Global Tracker",
        }
    }
}

struct Inspector {
    handle: backend::Handle,

    open_tabs: u8,

    globals: GlobalsStore,
    profiler: Profiler,

    object_creator: WindowedTool<ObjectCreator>,
    metadata_editor: WindowedTool<MetadataEditor>,
    context_manager: WindowedTool<ContextManager>,
}

impl Inspector {
    pub fn new(
        remote: RemoteInfo,
        mainloop_properties: Vec<(String, String)>,
        context_properties: Vec<(String, String)>,
    ) -> Self {
        Self {
            handle: backend::Handle::run(remote, mainloop_properties, context_properties),

            open_tabs: View::GlobalTracker as u8,

            globals: GlobalsStore::new(),
            profiler: Profiler::with_max_profilings(250),

            object_creator: WindowedTool::default(),
            metadata_editor: WindowedTool::default(),
            context_manager: WindowedTool::default(),
        }
    }

    pub fn views_menu_buttons(&mut self, ui: &mut egui::Ui, tree: &mut egui_dock::Tree<View>) {
        ui.menu_button("View", |ui| {
            for (tab, title, description) in [
                (
                    View::GlobalTracker,
                    "📑 Global Tracker",
                    "List of all the objects in the remote",
                ),
                (View::Profiler, "📈 Profiler", "Graphs of profiling data"),
                (
                    View::ProcessViewer,
                    "⏱ Process Viewer",
                    "Performance measurements of running nodes",
                ),
            ] {
                let bit = tab as u8;
                ui.add_enabled_ui(self.open_tabs & bit == 0, |ui| {
                    if ui
                        .selectable_label(self.open_tabs & bit != 0, title)
                        .on_hover_text(description)
                        .clicked()
                    {
                        self.open_tabs |= bit;
                        tree.push_to_focused_leaf(tab);
                    }
                });
            }
        });
    }

    pub fn tools_menu_buttons(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Tools", |ui| {
            for (open, name, description) in [
                (
                    &mut self.object_creator.open,
                    "⛭ Object Creator",
                    "Create an object on the remote",
                ),
                (
                    &mut self.metadata_editor.open,
                    "🗐 Metadata Editor",
                    "Edit remote metadata",
                ),
                (
                    &mut self.context_manager.open,
                    "🗄 Context Manager",
                    "Manage the PipeWire context",
                ),
            ] {
                ui.toggle_value(open, name).on_hover_text(description);
            }
        });
    }

    pub fn tool_windows(&mut self, ctx: &egui::Context) {
        self.object_creator.window(ctx, &self.handle.sx);
        self.metadata_editor.window(ctx, &self.handle.sx);
        self.context_manager.window(ctx, &self.handle.sx);
    }

    #[must_use = "Indicates whether the connection to the backend has ended"]
    pub fn process_events_or_stop(&mut self) -> bool {
        while let Ok(e) = self.handle.rx.try_recv() {
            match e {
                Event::Stop => return true,
                e => self.process_event(e),
            }
        }

        false
    }

    fn process_event(&mut self, e: Event) {
        match e {
            Event::GlobalAdded(id, object_type, props) => {
                let global = self.globals.add_global(id, object_type, props).borrow();

                if global.props().is_empty() {
                    return;
                }

                match *global.object_type() {
                    ObjectType::Factory => {
                        if let (Some(name), Some(object_type)) =
                            (global.name(), global.props().get("factory.type.name"))
                        {
                            let object_type = match object_type.as_str() {
                                "PipeWire:Interface:Link" => ObjectType::Link,
                                "PipeWire:Interface:Port" => ObjectType::Port,
                                "PipeWire:Interface:Node" => ObjectType::Node,
                                "PipeWire:Interface:Client" => ObjectType::Client,
                                "PipeWire:Interface:Device" => ObjectType::Device,
                                "PipeWire:Interface:Registry" => ObjectType::Registry,
                                "PipeWire:Interface:Profiler" => ObjectType::Profiler,
                                "PipeWire:Interface:Metadata" => ObjectType::Metadata,
                                "PipeWire:Interface:Factory" => ObjectType::Factory,
                                "PipeWire:Interface:Module" => ObjectType::Module,
                                "PipeWire:Interface:Core" => ObjectType::Core,
                                "PipeWire:Interface:Endpoint" => ObjectType::Endpoint,
                                "PipeWire:Interface:EndpointLink" => ObjectType::EndpointLink,
                                "PipeWire:Interface:EndpointStream" => ObjectType::EndpointStream,
                                "PipeWire:Interface:ClientSession" => ObjectType::ClientSession,
                                "PipeWire:Interface:ClientEndpoint" => ObjectType::ClientEndpoint,
                                "PipeWire:Interface:ClientNode" => ObjectType::ClientNode,
                                _ => ObjectType::Other(object_type.clone()),
                            };
                            self.object_creator.tool.add_factory(id, name, object_type);
                        }
                    }
                    ObjectType::Metadata => {
                        if let Some(name) = global.name() {
                            self.metadata_editor.tool.add_metadata(id, name);
                        }
                    }
                    _ => {}
                }
            }
            Event::GlobalRemoved(id) => {
                if let Some(removed) = self.globals.remove_global(id) {
                    match *removed.borrow().object_type() {
                        ObjectType::Metadata => {
                            self.metadata_editor.tool.remove_metadata(id);
                        }
                        ObjectType::Factory => {
                            self.object_creator.tool.remove_factory(id);
                        }
                        _ => {}
                    }
                }
            }
            Event::GlobalInfo(id, info) => {
                self.globals.set_global_info(id, Some(info));
            }
            Event::GlobalProperties(id, props) => {
                self.globals.set_global_props(id, props);
            }
            Event::ProfilerProfile(samples) => {
                self.profiler.add_profilings(samples);
            }
            Event::MetadataProperty {
                id,
                subject,
                key,
                type_,
                value,
            } => match key {
                Some(key) => match value {
                    Some(value) => {
                        let Some(metadata) = self.globals.get_global(id) else {
                            return;
                        };
                        self.metadata_editor.tool.add_property(
                            id,
                            metadata
                                .borrow()
                                .name()
                                .cloned()
                                .unwrap_or_else(|| format!("Unnamed metadata {id}")),
                            subject,
                            key,
                            type_,
                            value,
                        );
                    }
                    None => {
                        self.metadata_editor.tool.remove_property(id, &key);
                    }
                },
                None => {
                    self.metadata_editor.tool.clear_properties(id);
                }
            },
            Event::ClientPermissions(id, _, perms) => {
                if let Some(global) = self.globals.get_global(id) {
                    if let ObjectData::Client { permissions, .. } =
                        global.borrow_mut().object_data_mut()
                    {
                        *permissions = Some(perms);
                    }
                }
            }
            Event::ContextProperties(properties) => {
                self.context_manager.tool.set_context_properties(properties);
            }
            Event::Stop => unreachable!(),
        }
    }
}

impl egui_dock::TabViewer for Inspector {
    type Tab = View;

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match *tab {
            View::Profiler => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.profiler.show_profiler(ui);
                });
            }
            View::ProcessViewer => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.profiler.show_process_viewer(ui);
                });
            }
            View::GlobalTracker => {
                self.globals.show(ui, &self.handle.sx);
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.as_str().into()
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> bool {
        self.open_tabs &= !(*tab as u8);
        true
    }
}

enum State {
    Connected {
        tabs_tree: egui_dock::Tree<View>,
        inspector: Inspector,
        about: bool,
    },
    Unconnected {
        remote: RemoteInfo,
        mainloop_properties: EditableKVList,
        context_properties: EditableKVList,
    },
}

impl State {
    pub fn unconnected_from_env() -> Self {
        let mut context_properties = EditableKVList::new();
        context_properties
            .list_mut()
            .push(("media.category".to_owned(), "Manager".to_owned()));

        Self::Unconnected {
            remote: RemoteInfo::default(),
            mainloop_properties: EditableKVList::new(),
            context_properties,
        }
    }

    pub fn new_connected(
        remote: RemoteInfo,
        mainloop_properties: Vec<(String, String)>,
        context_properties: Vec<(String, String)>,
    ) -> Self {
        let mut tabs = Vec::with_capacity(3 /* Number of views */);
        tabs.push(View::GlobalTracker);

        Self::Connected {
            tabs_tree: egui_dock::Tree::new(tabs),
            inspector: Inspector::new(remote, mainloop_properties, context_properties),
            about: false,
        }
    }

    pub fn connect(&mut self) {
        if let Self::Unconnected {
            remote,
            mainloop_properties,
            context_properties,
        } = self
        {
            *self = Self::new_connected(
                std::mem::take(remote),
                mainloop_properties.take(),
                context_properties.take(),
            );
        }
    }

    pub fn disconnect(&mut self) {
        if let Self::Connected { .. } = self {
            *self = Self::unconnected_from_env();
        }
    }
}

pub struct CoppwrApp(State);

impl CoppwrApp {
    pub fn new() -> Self {
        Self(State::new_connected(
            RemoteInfo::default(),
            Vec::new(),
            vec![("media.category".to_owned(), "Manager".to_owned())],
        ))
    }
}

impl eframe::App for CoppwrApp {
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.0.disconnect();
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // egui won't update until there is interaction so data shown may be out of date
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        match &mut self.0 {
            State::Connected {
                tabs_tree,
                inspector,
                about,
            } => {
                if inspector.process_events_or_stop() {
                    self.0.disconnect();
                    return;
                }

                let mut disconnect = false;
                egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            disconnect = ui
                                .button("🔌 Disconnect")
                                .on_hover_text("Disconnect from the PipeWire remote")
                                .clicked();

                            ui.separator();

                            if ui.button("❌ Quit").clicked() {
                                frame.close();
                            }
                        });

                        inspector.views_menu_buttons(ui, tabs_tree);
                        inspector.tools_menu_buttons(ui);

                        ui.menu_button("Help", |ui| {
                            if ui.button("❓ About").clicked() {
                                *about = true;
                            }
                        })
                    });
                });

                if disconnect {
                    self.0.disconnect();
                    return;
                }

                egui::Window::new("About")
                    .collapsible(false)
                    .fixed_size([350f32, 150f32])
                    .default_pos([
                        (frame.info().window_info.size.x - 350f32) / 2f32,
                        (frame.info().window_info.size.y - 150f32) / 2f32,
                    ])
                    .open(about)
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.heading(env!("CARGO_PKG_NAME"));
                            ui.label(env!("CARGO_PKG_VERSION"));
                            ui.label(env!("CARGO_PKG_DESCRIPTION"));

                            ui.separator();

                            ui.label("2023 Dimitris Papaioannou");
                            ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));

                            ui.separator();

                            ui.label("This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License version 3 as published by the Free Software Foundation.");
                        });
                    });

                inspector.tool_windows(ctx);

                let mut style = egui_dock::Style::from_egui(ctx.style().as_ref());
                style.tabs.inner_margin = egui::Margin::symmetric(5., 5.);
                egui_dock::DockArea::new(tabs_tree)
                    .style(style)
                    .scroll_area_in_tabs(false)
                    .show(ctx, inspector);
            }
            State::Unconnected {
                remote,
                mainloop_properties,
                context_properties,
            } => {
                let mut connect = false;
                egui::CentralPanel::default().show(ctx, |_| {});
                egui::Window::new("Connect to PipeWire")
                    .fixed_size([300., 200.])
                    .default_pos([
                        (frame.info().window_info.size.x - 300.) / 2.,
                        (frame.info().window_info.size.y - 200.) / 2.,
                    ])
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.with_layout(egui::Layout::default().with_cross_justify(true), |ui| {
                            #[cfg(feature = "xdg_desktop_portals")]
                            egui::ComboBox::new("remote_type", "Remote kind")
                                .selected_text({
                                    match remote {
                                        RemoteInfo::Regular(..) => "Regular",
                                        RemoteInfo::Screencast { .. } => "Screencast portal",
                                        RemoteInfo::Camera => "Camera portal",
                                    }
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(remote, RemoteInfo::default(), "Regular");
                                    ui.selectable_value(
                                        remote,
                                        RemoteInfo::Screencast {
                                            types: BitFlags::EMPTY,
                                            multiple: false,
                                        },
                                        "Screencast portal",
                                    );
                                    ui.selectable_value(
                                        remote,
                                        RemoteInfo::Camera,
                                        "Camera portal",
                                    );
                                });

                            match remote {
                                RemoteInfo::Regular(name) => {
                                    egui::TextEdit::singleline(name)
                                        .hint_text("Remote name")
                                        .show(ui);
                                }

                                #[cfg(feature = "xdg_desktop_portals")]
                                RemoteInfo::Screencast { types, multiple } => {
                                    ui.horizontal(|ui| {
                                        ui.label("Source types");
                                        for (label, source_type) in [
                                            ("Monitor", SourceType::Monitor),
                                            ("Window", SourceType::Window),
                                            ("Virtual", SourceType::Virtual),
                                        ] {
                                            if ui
                                                .selectable_label(
                                                    types.contains(source_type),
                                                    label,
                                                )
                                                .clicked()
                                            {
                                                types.toggle(source_type);
                                            }
                                        }
                                    });
                                    ui.checkbox(multiple, "Multiple sources");
                                }
                                #[cfg(feature = "xdg_desktop_portals")]
                                RemoteInfo::Camera => {}
                            }
                        });

                        ui.separator();

                        for (heading, properties) in [
                            ("Mainloop properties", mainloop_properties),
                            ("Context properties", context_properties),
                        ] {
                            egui::CollapsingHeader::new(heading)
                                .show_unindented(ui, |ui| properties.show(ui));
                        }

                        ui.separator();

                        ui.with_layout(
                            egui::Layout::top_down_justified(egui::Align::Center),
                            |ui| {
                                connect = ui.button("Connect").clicked();
                            },
                        );
                    });

                if connect {
                    self.0.connect();
                }
            }
        }
    }
}
