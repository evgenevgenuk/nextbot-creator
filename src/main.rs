#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod generator;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, ColorImage, FontId, Rect, RichText, Rounding, Sense, Stroke,
    TextureHandle, TextureOptions,
};
use generator::{ext_lower, is_gif, NextbotSpec, IMAGE_EXTS, SOUND_EXTS};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BACKGROUND_IMG: &[u8] = include_bytes!("../assets/background.png");
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Image,
    Chase,
    Jump,
    Attack,
}

impl Slot {
    const ALL: [Slot; 4] = [Slot::Image, Slot::Chase, Slot::Jump, Slot::Attack];

    fn label(self) -> &'static str {
        match self {
            Slot::Image => "Image Nextbot",
            Slot::Chase => "Sound Chase",
            Slot::Jump => "Sound Jump",
            Slot::Attack => "Sound Attack",
        }
    }

    fn exts(self) -> &'static [&'static str] {
        if self == Slot::Image { IMAGE_EXTS } else { SOUND_EXTS }
    }
}

enum Preview {
    None,
    Still(TextureHandle),
    Anim { frames: Vec<TextureHandle>, delays: Vec<f32>, total: f32 },
}

enum Status {
    None,
    Ok(String, PathBuf),
    Err(String),
}

struct App {
    background: Option<TextureHandle>,
    name: String,
    paths: [Option<PathBuf>; 4],
    physgun_disabled: bool,
    admin_only: bool,
    preview: Preview,
    status: Status,
    zone_rects: [Rect; 4],
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let background = image::load_from_memory(BACKGROUND_IMG).ok().map(|img| {
            cc.egui_ctx.load_texture("background", to_color_image(&img.to_rgba8()), TextureOptions::LINEAR)
        });
        Self {
            background,
            name: String::new(),
            paths: [None, None, None, None],
            physgun_disabled: true,
            admin_only: true,
            preview: Preview::None,
            status: Status::None,
            zone_rects: [Rect::NOTHING; 4],
        }
    }

    fn set_path(&mut self, ctx: &egui::Context, slot: Slot, path: PathBuf) {
        if !slot.exts().contains(&ext_lower(&path).as_str()) {
            self.status = Status::Err(format!(
                "{}: .{} is not supported (use {})",
                slot.label(),
                ext_lower(&path),
                slot.exts().join(", ")
            ));
            return;
        }
        if slot == Slot::Image {
            match load_preview(ctx, &path) {
                Ok(p) => self.preview = p,
                Err(e) => {
                    self.status = Status::Err(format!("Image: {e:#}"));
                    return;
                }
            }
        }
        self.paths[slot as usize] = Some(path);
        self.status = Status::None;
    }

    fn pick_file(&mut self, ctx: &egui::Context, slot: Slot) {
        let kind = if slot == Slot::Image { "Image File" } else { "Audio File" };
        if let Some(p) = rfd::FileDialog::new().add_filter(kind, slot.exts()).pick_file() {
            self.set_path(ctx, slot, p);
        }
    }

    /// Dropped files go to the zone under the cursor, or else by file type.
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let (dropped, pointer) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.latest_pos()));
        for file in dropped {
            let Some(path) = file.path else { continue };
            let under_cursor = pointer.and_then(|p| {
                Slot::ALL.into_iter().find(|s| self.zone_rects[*s as usize].contains(p))
            });
            let slot = under_cursor.unwrap_or_else(|| {
                if IMAGE_EXTS.contains(&ext_lower(&path).as_str()) {
                    Slot::Image
                } else {
                    [Slot::Chase, Slot::Jump, Slot::Attack]
                        .into_iter()
                        .find(|s| self.paths[*s as usize].is_none())
                        .unwrap_or(Slot::Chase)
                }
            });
            self.set_path(ctx, slot, path);
        }
    }

    fn create(&mut self) {
        let [Some(image), Some(chase), Some(jump), Some(attack)] = self.paths.clone() else {
            self.status = Status::Err("Please fill in all fields.".into());
            return;
        };
        if self.name.trim().is_empty() {
            self.status = Status::Err("Please fill in all fields.".into());
            return;
        }
        let spec = NextbotSpec {
            name: self.name.trim().to_string(),
            image,
            chase_sound: chase,
            jump_sound: jump,
            attack_sound: attack,
            physgun_disabled: self.physgun_disabled,
            admin_only: self.admin_only,
        };
        let out_root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        self.status = match generator::create_nextbot(&spec, &out_root) {
            Ok(addon) => Status::Ok(format!("Nextbot \"{}\" has been successfully created!", spec.name), addon),
            Err(e) => Status::Err(format!("{e:#}")),
        };
    }

    fn drop_zone(&mut self, ui: &mut egui::Ui, slot: Slot, dragging: bool) {
        ui.label(RichText::new(slot.label()).size(15.0).color(Color32::WHITE));
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(300.0, 26.0), Sense::click());
            self.zone_rects[slot as usize] = rect;
            let path = &self.paths[slot as usize];
            let hovered_by_drag = dragging && ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p));
            let fill = if hovered_by_drag {
                Color32::from_gray(200)
            } else if path.is_some() {
                Color32::from_gray(110)
            } else if resp.hovered() {
                Color32::from_gray(245)
            } else {
                Color32::WHITE
            };
            ui.painter().rect(rect, Rounding::same(4.0), fill, Stroke::new(1.0f32, Color32::from_gray(60)));
            let (text, color) = match path {
                Some(p) => (
                    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    Color32::WHITE,
                ),
                None => ("drop a file here or click".to_string(), Color32::from_gray(120)),
            };
            ui.painter().with_clip_rect(rect.shrink(4.0)).text(
                rect.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(13.0),
                color,
            );
            let resp = resp.on_hover_text(path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| slot.exts().join(", ")));
            if resp.clicked() || ui.button("...").clicked() {
                self.pick_file(ui.ctx(), slot);
            }
        });
        ui.add_space(4.0);
    }
}

fn load_preview(ctx: &egui::Context, path: &Path) -> anyhow::Result<Preview> {
    if is_gif(path) {
        let frames = generator::decode_gif(path)?;
        let delays: Vec<f32> = frames.iter().map(|f| f.delay_secs).collect();
        let total = delays.iter().sum();
        let frames = frames
            .iter()
            .enumerate()
            .map(|(i, f)| ctx.load_texture(format!("preview{i}"), to_color_image(&f.image), TextureOptions::LINEAR))
            .collect();
        Ok(Preview::Anim { frames, delays, total })
    } else {
        let img = image::open(path)?.to_rgba8();
        Ok(Preview::Still(ctx.load_texture("preview", to_color_image(&img), TextureOptions::LINEAR)))
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_drops(ctx);
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let full = ui.max_rect();
            if let Some(bg) = &self.background {
                // "Cover" fit: fill the window, crop the overflow, keep proportions.
                let img_aspect = bg.aspect_ratio();
                let win_aspect = full.width() / full.height();
                let uv = if win_aspect > img_aspect {
                    let h = img_aspect / win_aspect;
                    Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
                } else {
                    let w = win_aspect / img_aspect;
                    Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
                };
                ui.painter().image(bg.id(), full, uv, Color32::WHITE);
            }
            ui.painter().rect_filled(full, 0.0, Color32::from_black_alpha(150));

            egui::Frame::none().inner_margin(16.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Name Nextbot").size(15.0).color(Color32::WHITE));
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(300.0).hint_text("e.g. Obunga"));
                        ui.add_space(4.0);
                        for slot in Slot::ALL {
                            self.drop_zone(ui, slot, dragging);
                        }
                        ui.checkbox(&mut self.physgun_disabled, RichText::new("Physgun Disabled").size(15.0));
                        ui.checkbox(&mut self.admin_only, RichText::new("Admin Only").size(15.0));
                    });
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.add_space(24.0);
                        let (rect, _) = ui.allocate_exact_size(vec2(128.0, 128.0), Sense::hover());
                        ui.painter().rect_filled(rect, 4.0, Color32::from_black_alpha(120));
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        match &self.preview {
                            Preview::None => {
                                ui.painter().text(rect.center(), Align2::CENTER_CENTER, "no image", FontId::proportional(13.0), Color32::GRAY);
                            }
                            Preview::Still(t) => {
                                ui.painter().image(t.id(), rect, uv, Color32::WHITE);
                            }
                            Preview::Anim { frames, delays, total } => {
                                let mut t = (ui.input(|i| i.time) as f32) % total.max(0.001);
                                let mut idx = frames.len() - 1;
                                for (i, d) in delays.iter().enumerate() {
                                    t -= d;
                                    if t < 0.0 {
                                        idx = i;
                                        break;
                                    }
                                }
                                ui.painter().image(frames[idx].id(), rect, uv, Color32::WHITE);
                                ui.label(RichText::new(format!("GIF, {} frames", frames.len())).size(12.0).color(Color32::LIGHT_GRAY));
                                ctx.request_repaint_after(Duration::from_millis(15));
                            }
                        }
                    });
                });

                ui.add_space(8.0);
                if ui.add_sized([150.0, 34.0], egui::Button::new(RichText::new("Create").size(18.0))).clicked() {
                    self.create();
                }
                ui.add_space(4.0);
                let mut open = None;
                match &self.status {
                    Status::None => {}
                    Status::Ok(msg, addon) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN));
                            if ui.button("Open folder").clicked() {
                                open = Some(addon.clone());
                            }
                        });
                    }
                    Status::Err(msg) => {
                        ui.label(RichText::new(msg).color(Color32::from_rgb(255, 120, 120)));
                    }
                }
                if let Some(dir) = open {
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
            });
        });
    }
}

fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(ICON_PNG).expect("embedded icon is a valid PNG").to_rgba8();
    egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 520.0])
            .with_resizable(false)
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Simple Nextbot Creator",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
