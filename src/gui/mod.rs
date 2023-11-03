mod backend;
mod donut;
mod egui_backend;
mod icons;
mod style;
mod texture;
mod utils;
mod winit_backend;

use egui::text::LayoutJob;
use egui_dock::tree::Tree;
use once_cell::sync::{Lazy, OnceCell};
use pollster::FutureExt;
use tokenizing::Token;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};

use crate::disassembly::{Disassembly, DisassemblyView};
use crate::terminal::Terminal;
use backend::Backend;
use debugger::{Debugger, Process};
use egui::{Button, RichText, FontId};
use egui_backend::Pipeline;
use winit_backend::{CustomEvent, Platform, PlatformDescriptor};

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

pub static WINDOW: OnceCell<Arc<winit::window::Window>> = OnceCell::new();
pub static STYLE: Lazy<style::Style> = Lazy::new(style::Style::default);

const LIST_FONT: FontId = egui::FontId::new(14.0, egui::FontFamily::Monospace);

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 800;

const DISASS_TITLE: &str = crate::icon!(PARAGRAPH_LEFT, " Disassembly");
const FUNCS_TITLE: &str = crate::icon!(LIGATURE, " Functions");
const SOURCE_TITLE: &str = crate::icon!(EMBED2, " Source");
const LOG_TITLE: &str = crate::icon!(TERMINAL, " Logs");

type Title = &'static str;
type DisassThread = JoinHandle<Result<Disassembly, crate::disassembly::DecodeError>>;

pub enum Error {
    WindowCreation,
    SurfaceCreation(wgpu::CreateSurfaceError),
    AdapterRequest,
    DeviceRequest(wgpu::RequestDeviceError),
    InvalidTextureId(egui::TextureId),
    PngDecode,
    PngFormat,
    NotFound(std::path::PathBuf),
    Exit,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowCreation => f.write_str("Failed to create a window."),
            Self::SurfaceCreation(..) => f.write_str("Failed to create a surface."),
            Self::AdapterRequest => {
                f.write_str("Failed to find a adapter that supports our surface.")
            }
            Self::DeviceRequest(..) => {
                f.write_str("Failed to find a device that meets our adapter's limits.")
            }
            Self::InvalidTextureId(id) => {
                f.write_fmt(format_args!("Egui texture id '{id:?}' was invalid."))
            }
            Self::PngDecode => f.write_str("Invalid data given to the png decoder."),
            Self::PngFormat => {
                f.write_str("Unsupported texture format produced by the png decoder.")
            }
            Self::NotFound(path) => {
                f.write_fmt(format_args!("Failed to find path: '{}'", path.display()))
            }
            Self::Exit => Ok(())
        }
    }
}

pub fn tokens_to_layoutjob(tokens: Vec<Token>) -> LayoutJob {
    let mut job = LayoutJob::default();

    for token in tokens {
        job.append(
            &token.text,
            0.0,
            egui::TextFormat {
                font_id: LIST_FONT,
                color: token.color,
                ..Default::default()
            },
        );
    }

    job
}

pub struct RenderContext {
    panels: Tree<Title>,
    pub buffers: Buffers,

    style: style::Style,
    window: Arc<winit::window::Window>,
    donut: donut::Donut,
    show_donut: Arc<AtomicBool>,
    timer60: utils::Timer,
    pub dissasembly: Option<Arc<Disassembly>>,
    disassembling_thread: Option<DisassThread>,

    #[cfg(target_family = "windows")]
    unwindowed_size: winit::dpi::PhysicalSize<u32>,
    #[cfg(target_family = "windows")]
    unwindowed_pos: winit::dpi::PhysicalPosition<i32>,

    terminal: Terminal,

    pub process_path: Option<std::path::PathBuf>,
    pub terminal_prompt: String,
}

impl RenderContext {
    pub fn start_disassembling(&mut self, path: impl AsRef<std::path::Path> + 'static + Send) {
        let show_donut = Arc::clone(&self.show_donut);

        self.process_path = Some(path.as_ref().to_path_buf());
        self.disassembling_thread = Some(std::thread::spawn(move || {
            Disassembly::parse(path, show_donut)
        }));
    }

    pub fn start_debugging(
        &mut self,
        path: impl AsRef<std::path::Path> + 'static + Send,
        args: Vec<String>,
    ) {
        #[cfg(target_os = "linux")]
        std::thread::spawn(move || {
            // the debugger must not be moved to a different thread,
            // not sure why this is the case
            let mut session = Debugger::spawn(path, args).unwrap();

            session.trace_syscalls(true);
            session.run_to_end().unwrap();
        });
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TabKind {
    Source,
    Listing,
    Functions,
    Log,
}

pub struct Buffers {
    mapping: HashMap<Title, TabKind>,
    disassembly: Option<Arc<Disassembly>>,
    disassembly_view: DisassemblyView,

    diss_text: LayoutJob,
    diss_min_row: usize,
    diss_max_row: usize,

    funcs_text: LayoutJob,
    funcs_min_row: usize,
    funcs_max_row: usize,
}

impl Buffers {
    fn new(mapping: HashMap<Title, TabKind>) -> Self {
        Self {
            mapping,
            disassembly: None,
            disassembly_view: DisassemblyView::new(),
            diss_text: LayoutJob::default(),
            diss_min_row: 0,
            diss_max_row: 0,
            funcs_text: LayoutJob::default(),
            funcs_min_row: 0,
            funcs_max_row: 0,
        }
    }

    pub fn listing_jump(&mut self, addr: usize) -> bool {
        let disassembly = match self.disassembly {
            Some(ref dissasembly) => dissasembly,
            None => return false,
        };

        if !self.disassembly_view.jump(disassembly, addr) {
            return false;
        }

        self.diss_text = self.disassembly_view.format();
        true
    }

    fn show_listing(&mut self, ui: &mut egui::Ui) {
        let disassembly = match self.disassembly {
            Some(ref dissasembly) => dissasembly,
            None => return,
        };

        if let Some(text) = disassembly.section(self.disassembly_view.addr) {
            let max_width = ui.available_width();
            let size = egui::vec2(9.0 * text.len() as f32, 25.0);
            let offset = egui::pos2(20.0, 60.0);
            let rect = egui::Rect::from_two_pos(
                egui::pos2(max_width - offset.x, offset.y),
                egui::pos2(max_width - offset.x - size.x, offset.y + size.y),
            );

            ui.painter().rect(
                rect.expand2(egui::vec2(5.0, 0.0)),
                0.0,
                tokenizing::colors::GRAY35,
                egui::Stroke::new(2.5, egui::Color32::BLACK),
            );

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                LIST_FONT,
                egui::Color32::WHITE,
            );
        }

        let spacing = ui.spacing().item_spacing;
        let row_height_with_spacing = LIST_FONT.size + spacing.y;

        let area = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);

        area.show_viewport(ui, |ui, viewport| {
            let min_row = (viewport.min.y / row_height_with_spacing).floor() as usize;
            let max_row = (viewport.max.y / row_height_with_spacing).ceil() as usize + 1;

            let y_min = ui.max_rect().top() + min_row as f32 * row_height_with_spacing;
            let y_max = ui.max_rect().top() + max_row as f32 * row_height_with_spacing;

            let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), y_min..=y_max);

            ui.allocate_ui_at_rect(rect, |ui| {
                ui.skip_ahead_auto_ids(min_row);

                if (min_row..max_row) != (self.diss_min_row..self.diss_min_row) {
                    let row_count = max_row - min_row;
                    self.disassembly_view.set_max_lines(row_count * 2, disassembly);

                    // initial rendering of listing
                    if min_row == 0 {
                        self.diss_text = self.disassembly_view.format();
                        self.diss_min_row = min_row;
                        self.diss_max_row = max_row;
                    }
                }

                if min_row != self.diss_min_row {
                    if min_row > self.diss_min_row {
                        let row_diff = min_row - self.diss_min_row;
                        self.disassembly_view.scroll_down(disassembly, row_diff);
                    }

                    if min_row < self.diss_min_row {
                        let row_diff = self.diss_min_row - min_row;
                        self.disassembly_view.scroll_up(disassembly, row_diff);
                    }

                    self.diss_text = self.disassembly_view.format();
                    self.diss_min_row = min_row;
                    self.diss_max_row = max_row;
                }

                ui.label(self.diss_text.clone());
            });
        });
    }

    fn show_functions(&mut self, ui: &mut egui::Ui) {
        let dissasembly = match self.disassembly {
            Some(ref dissasembly) => dissasembly,
            None => return,
        };

        let text_style = egui::TextStyle::Small;
        let row_height = ui.text_style_height(&text_style);
        let total_rows = dissasembly.symbols.named_len();

        let area = egui::ScrollArea::both().auto_shrink([false, false]).drag_to_scroll(false);

        area.show_rows(ui, row_height, total_rows, |ui, row_range| {
            if row_range != (self.funcs_min_row..self.funcs_max_row) {
                self.funcs_text = dissasembly.functions(row_range);
            }

            ui.label(self.funcs_text.clone());
        });
    }

    fn show_logger(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().wrap = Some(true);

        let area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .stick_to_bottom(true);

        area.show(ui, |ui| ui.label(log::LOGGER.lock().unwrap().format()));

        ui.style_mut().wrap = Some(false);
    }
}

impl egui_dock::TabViewer for Buffers {
    type Tab = Title;

    fn ui(&mut self, ui: &mut egui::Ui, title: &mut Self::Tab) {
        egui::Frame::none().outer_margin(STYLE.separator_width).show(ui, |ui| {
            match self.mapping.get(title) {
                Some(TabKind::Source) => {
                    ui.label("todo");
                }
                Some(TabKind::Functions) => self.show_functions(ui),
                Some(TabKind::Listing) => self.show_listing(ui),
                Some(TabKind::Log) => self.show_logger(ui),
                None => return,
            };
        });
    }

    fn title(&mut self, title: &mut Self::Tab) -> egui::WidgetText {
        (*title).into()
    }
}

fn top_bar(ui: &mut egui::Ui, ctx: &mut RenderContext, platform: &mut Platform) {
    let bar = egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button(crate::icon!(FOLDER_OPEN, " Open")).clicked() {
                backend::ask_for_binary(ctx);
                ui.close_menu();
            }

            if ui.button(crate::icon!(CROSS, " Exit")).clicked() {
                platform.send_event(CustomEvent::CloseRequest);
                ui.close_menu();
            }
        });

        ui.menu_button("Windows", |ui| {
            let mut goto_window = |title| match ctx.panels.find_tab(&title) {
                Some((node_idx, tab_idx)) => ctx.panels.set_active_tab(node_idx, tab_idx),
                None => ctx.panels.push_to_first_leaf(title),
            };

            if ui.button(DISASS_TITLE).clicked() {
                goto_window(DISASS_TITLE);
                ui.close_menu();
            }

            if ui.button(SOURCE_TITLE).clicked() {
                goto_window(SOURCE_TITLE);
                ui.close_menu();
            }

            if ui.button(FUNCS_TITLE).clicked() {
                goto_window(FUNCS_TITLE);
                ui.close_menu();
            }

            if ui.button(LOG_TITLE).clicked() {
                goto_window(LOG_TITLE);
                ui.close_menu();
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            top_bar_native(ui, platform, ctx);
        });
    });

    if bar.response.interact(egui::Sense::click()).double_clicked() {
        backend::fullscreen(ctx);
    }

    if bar.response.interact(egui::Sense::drag()).dragged() {
        platform.start_dragging();
    } else {
        platform.stop_dragging();
    }
}

/// Show some close/maximize/minimize buttons for the native window.
fn top_bar_native(ui: &mut egui::Ui, platform: &mut Platform, ctx: &mut RenderContext) {
    let height = 12.0;
    let close_response = ui.add(Button::new(RichText::new(crate::icon!(CROSS)).size(height)));

    if close_response.clicked() {
        platform.send_event(CustomEvent::CloseRequest);
    }

    let maximized_response = ui.add(Button::new(
        RichText::new(crate::icon!(CHECKBOX_UNCHECKED)).size(height),
    ));

    if maximized_response.clicked() {
        backend::fullscreen(ctx);
    }

    let minimized_response = ui.add(Button::new(RichText::new(crate::icon!(MINUS)).size(height)));

    if minimized_response.clicked() {
        ctx.window.set_minimized(true);
    }
}

fn tabbed_panel(ui: &mut egui::Ui, ctx: &mut RenderContext) {
    let tab_count = ctx.panels.num_tabs();

    egui_dock::DockArea::new(&mut ctx.panels)
        .style(ctx.style.dock().clone())
        .draggable_tabs(tab_count > 1)
        .show_inside(ui, &mut ctx.buffers);
}

fn terminal(ui: &mut egui::Ui, ctx: &mut RenderContext) {
    ui.style_mut().wrap = Some(true);

    let area = egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false);

    area.show(ui, |ui| {
        let mut output = LayoutJob::default();

        let text_style = egui::TextStyle::Body;
        let font_id = text_style.resolve(STYLE.egui());

        output.append(
            &ctx.terminal_prompt,
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: STYLE.egui().noninteractive().fg_stroke.color,
                ..Default::default()
            },
        );

        output.append(
            "(bite) ",
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: STYLE.egui().noninteractive().fg_stroke.color,
                ..Default::default()
            },
        );

        ctx.terminal.format(&mut output, font_id);
        ui.label(output);
    });

    ui.style_mut().wrap = Some(false);
}

pub fn init() -> Result<(), Error> {
    let event_loop = EventLoopBuilder::<CustomEvent>::with_user_event().build();

    let window = {
        #[cfg(target_os = "linux")]
        let decode = utils::decode_png_bytes(include_bytes!("../../assets/iconx64.png"));
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let decode = utils::decode_png_bytes(include_bytes!("../../assets/iconx256.png"));

        let mut icon = None;
        if let Ok(png) = decode {
            icon = winit::window::Icon::from_rgba(png.data, png.width, png.height).ok();
        }

        Arc::new(utils::generate_window("bite", icon, &event_loop)?)
    };

    WINDOW.set(Arc::clone(&window)).unwrap();

    let mut backend = Backend::new(&window).block_on()?;

    let mut egui_rpass = Pipeline::new(&backend.device, backend.surface_cfg.format, 1);
    let mut panels = Tree::new(vec![DISASS_TITLE, FUNCS_TITLE, LOG_TITLE]);

    panels.set_focused_node(egui_dock::NodeIndex::root());

    let buffers = HashMap::from([
        (DISASS_TITLE, TabKind::Listing),
        (FUNCS_TITLE, TabKind::Functions),
        (SOURCE_TITLE, TabKind::Source),
        (LOG_TITLE, TabKind::Log),
    ]);

    let mut ctx = RenderContext {
        panels,
        buffers: Buffers::new(buffers),
        style: STYLE.clone(),
        window: Arc::clone(&window),
        donut: donut::Donut::new(true),
        show_donut: Arc::new(AtomicBool::new(false)),
        timer60: utils::Timer::new(60),
        dissasembly: None,
        disassembling_thread: None,
        #[cfg(target_family = "windows")]
        unwindowed_size: window.outer_size(),
        #[cfg(target_family = "windows")]
        unwindowed_pos: window.outer_position().unwrap_or_default(),
        terminal: Terminal::new(),
        process_path: None,
        terminal_prompt: String::new(),
    };

    let mut platform = Platform::new(PlatformDescriptor {
        physical_width: 580,
        physical_height: 300,
        scale_factor: window.scale_factor() as f32,
        style: STYLE.egui().clone(),
        winit: event_loop.create_proxy(),
    });

    if let Some(ref path) = crate::ARGS.path {
        ctx.start_disassembling(path);
    }

    let start_time = Instant::now();

    event_loop.run(move |event, _, control| {
        // Pass the winit events to the platform integration
        platform.handle_event(&event);

        match event {
            Event::RedrawRequested(..) => {
                // update time elapsed
                platform.update_time(start_time.elapsed().as_secs_f64());

                // draw ui
                match backend.redraw(&mut ctx, &mut platform, &mut egui_rpass) {
                    Err(Error::Exit) => *control = ControlFlow::Exit,
                    Err(err) => crate::warning!("{err:?}"),
                    Ok(()) => {}
                }
            }
            Event::UserEvent(CustomEvent::CloseRequest) => *control = ControlFlow::Exit,
            Event::UserEvent(CustomEvent::DragWindow) => {
                let _ = ctx.window.drag_window();
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(size) => backend.resize(size),
                WindowEvent::CloseRequested => *control = ControlFlow::Exit,
                WindowEvent::DroppedFile(path) => ctx.start_disassembling(path),
                _ => {}
            },
            Event::MainEventsCleared => handle_post_render(&mut ctx),
            _ => {}
        }
    })
}

fn handle_post_render(ctx: &mut RenderContext) {
    if ctx.show_donut.load(Ordering::Relaxed) && ctx.timer60.reached() {
        ctx.donut.update_frame();
        ctx.timer60.reset();
    }

    // if there is a binary being loaded
    if let Some(true) = ctx.disassembling_thread.as_ref().map(JoinHandle::is_finished) {
        let thread = ctx.disassembling_thread.take().unwrap();

        // check if it's finished loading
        if thread.is_finished() {
            // store the loaded binary
            match thread.join() {
                Err(err) => {
                    ctx.show_donut.store(false, Ordering::Relaxed);
                    crate::warning!("{err:?}");
                }
                Ok(Err(err)) => {
                    ctx.show_donut.store(false, Ordering::Relaxed);
                    crate::warning!("{err:?}");
                }
                Ok(Ok(val)) => {
                    let dissasembly = Arc::new(val);

                    ctx.dissasembly = Some(Arc::clone(&dissasembly));
                    ctx.buffers.disassembly = Some(Arc::clone(&dissasembly));
                }
            }

            // mark the disassembling thread as not loading anything
            ctx.disassembling_thread = None;
        }
    }

    ctx.window.request_redraw();
}
