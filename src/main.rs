#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod resolve;
mod scene;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use glass_lens::glass::{self, Img, Params, LOOKS};
use std::path::PathBuf;

const PREVIEW_W: usize = 1280;
const PREVIEW_H: usize = 720;
const THUMB_W: usize = 160;
const THUMB_H: usize = 90;

const BG: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1f);
const PANEL: Color32 = Color32::from_rgb(0x24, 0x25, 0x29);
const FRAME: Color32 = Color32::from_rgb(0x2c, 0x2e, 0x32);
const ACCENT: Color32 = Color32::from_rgb(0xe8, 0x9a, 0x3c);
const TEXT: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd8);
const DIM: Color32 = Color32::from_rgb(0x8c, 0x8e, 0x94);

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut shot: Option<PathBuf> = None;
    let mut look: Option<usize> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--screenshot" if i + 1 < args.len() => {
                shot = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--look" if i + 1 < args.len() => {
                look = args[i + 1].parse().ok();
                i += 1;
            }
            "--export" if i + 1 < args.len() => {
                let looks: Vec<Params> = (0..LOOKS.len()).map(Params::preset).collect();
                match resolve::write_bundle(&PathBuf::from(&args[i + 1]), &looks) {
                    Ok(files) => {
                        for f in files {
                            println!("{}", f.display());
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("export failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 860.0])
        .with_min_inner_size([1000.0, 620.0])
        .with_title("Glass Lens Lab");
    if shot.is_some() {
        vp = vp.with_active(false);
        #[cfg(target_os = "windows")]
        {
            vp = vp.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport: vp, ..Default::default() };
    #[cfg(target_os = "macos")]
    if shot.is_some() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_activate_ignoring_other_apps(false);
        }));
    }
    eframe::run_native(
        "Glass Lens Lab",
        options,
        Box::new(move |cc| Ok(Box::new(LabApp::new(cc, shot, look)))),
    )
}

struct LabApp {
    full_src: Img,
    preview_src: Img,
    thumb_src: Img,
    source_name: String,
    looks: Vec<Params>,
    sel: usize,
    view_tex: Option<TextureHandle>,
    orig_tex: Option<TextureHandle>,
    thumbs: Vec<Option<TextureHandle>>,
    dirty: bool,
    thumbs_dirty: bool,
    compare: bool,
    show_name: bool,
    status: Vec<String>,
    shot: Option<PathBuf>,
    frame: u32,
    shot_sent: bool,
    installed: (bool, &'static str),
}

impl LabApp {
    fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>, look: Option<usize>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = Color32::from_rgb(0x18, 0x19, 0x1b);
        style.visuals.selection.bg_fill = ACCENT.linear_multiply(0.6);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x34, 0x36, 0x3b);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x34, 0x36, 0x3b);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x40, 0x42, 0x48);
        style.visuals.override_text_color = Some(TEXT);
        style.spacing.slider_width = 150.0;
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        cc.egui_ctx.set_style(style);

        let full = scene::sample_scene(1920, 1080);
        let mut app = LabApp {
            preview_src: full.fit(PREVIEW_W, PREVIEW_H),
            thumb_src: full.fit(THUMB_W, THUMB_H),
            full_src: full,
            source_name: "Sample shot (bar at night)".into(),
            looks: (0..LOOKS.len()).map(Params::preset).collect(),
            sel: look.unwrap_or(0).min(LOOKS.len() - 1),
            view_tex: None,
            orig_tex: None,
            thumbs: vec![None; LOOKS.len()],
            dirty: true,
            thumbs_dirty: true,
            compare: false,
            show_name: true,
            status: vec!["Pick a look on the left, tune it on the right, then install it into DaVinci Resolve.".into()],
            shot,
            frame: 0,
            shot_sent: false,
            installed: resolve::install_state(),
        };
        app.refresh(&cc.egui_ctx);
        app
    }

    fn set_source(&mut self, img: Img, name: String) {
        self.preview_src = img.fit(PREVIEW_W, PREVIEW_H);
        self.thumb_src = img.fit(THUMB_W, THUMB_H);
        self.full_src = img;
        self.source_name = name;
        self.orig_tex = None;
        self.dirty = true;
        self.thumbs_dirty = true;
    }

    fn load_picture(&mut self, path: PathBuf) {
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                let im = Img::from_rgba8(w, h, rgba.as_raw());
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.status = vec![format!("Loaded {name} ({w} x {h}).")];
                self.set_source(im, name);
            }
            Err(e) => self.status = vec![format!("Couldn't open that picture: {e}")],
        }
    }

    fn to_color_image(img: &Img) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &img.to_rgba8())
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.orig_tex.is_none() {
            self.orig_tex = Some(ctx.load_texture("orig", Self::to_color_image(&self.preview_src), TextureOptions::LINEAR));
        }
        if self.dirty {
            let out = glass::render(&self.preview_src, &self.looks[self.sel]);
            let ci = Self::to_color_image(&out);
            match &mut self.view_tex {
                Some(t) => t.set(ci, TextureOptions::LINEAR),
                None => self.view_tex = Some(ctx.load_texture("view", ci, TextureOptions::LINEAR)),
            }
            // keep the selected look's thumbnail in step
            let th = glass::render(&self.thumb_src, &self.looks[self.sel]);
            self.thumbs[self.sel] = Some(ctx.load_texture(format!("thumb{}", self.sel), Self::to_color_image(&th), TextureOptions::LINEAR));
            self.dirty = false;
        }
        if self.thumbs_dirty {
            for k in 0..LOOKS.len() {
                let th = glass::render(&self.thumb_src, &self.looks[k]);
                self.thumbs[k] = Some(ctx.load_texture(format!("thumb{k}"), Self::to_color_image(&th), TextureOptions::LINEAR));
            }
            self.thumbs_dirty = false;
        }
    }

    fn save_picture(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG picture", &["png"])
            .set_file_name(format!("glass-lens-{}.png", LOOKS[self.sel].to_lowercase().replace(' ', "-")))
            .save_file()
        else {
            return;
        };
        let out = glass::render(&self.full_src, &self.looks[self.sel]);
        let res = image::save_buffer(&path, &out.to_rgba8(), out.w as u32, out.h as u32, image::ExtendedColorType::Rgba8);
        self.status = vec![match res {
            Ok(()) => format!("Saved {}", path.display()),
            Err(e) => format!("Couldn't save: {e}"),
        }];
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(egui::RichText::new("LOOKS").size(12.0).color(DIM).strong());
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for k in 0..LOOKS.len() {
                let size = Vec2::new(THUMB_W as f32, THUMB_H as f32 + 24.0);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                let img_rect = Rect::from_min_size(rect.min, Vec2::new(THUMB_W as f32, THUMB_H as f32));
                let p = ui.painter();
                if k == self.sel {
                    p.rect_filled(img_rect.expand(3.0), 5.0, ACCENT);
                } else if resp.hovered() {
                    p.rect_filled(img_rect.expand(2.0), 5.0, Color32::from_gray(90));
                }
                if let Some(t) = &self.thumbs[k] {
                    p.image(t.id(), img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                p.text(
                    Pos2::new(rect.center().x, img_rect.max.y + 13.0),
                    Align2::CENTER_CENTER,
                    LOOKS[k],
                    FontId::proportional(14.0),
                    if k == self.sel { Color32::WHITE } else { TEXT },
                );
                if resp.clicked() && k != self.sel {
                    self.sel = k;
                    self.dirty = true;
                }
                ui.add_space(6.0);
            }
        });
    }

    fn slider(ui: &mut egui::Ui, label: &str, v: &mut f32, lo: f32, hi: f32, changed: &mut bool) {
        ui.label(label);
        if ui.add(egui::Slider::new(v, lo..=hi).max_decimals(2)).changed() {
            *changed = true;
        }
        ui.end_row();
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(egui::RichText::new("INSPECTOR").size(12.0).color(DIM).strong());
        ui.label(egui::RichText::new(LOOKS[self.sel]).size(20.0).color(Color32::WHITE));
        ui.add_space(6.0);
        let sel = self.sel;
        let mut changed = false;
        {
            let p = &mut self.looks[sel];
            let size_label = match sel {
                0 | 1 => "Clear centre",
                2 => "Facet size",
                3 => "Cell size",
                4 => "Edge position",
                _ => "Rib width",
            };
            ui.label(egui::RichText::new("Glass").color(ACCENT));
            egui::Grid::new("glass").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                Self::slider(ui, "Amount", &mut p.amount, 0.0, 1.0, &mut changed);
                Self::slider(ui, size_label, &mut p.size, 0.0, 1.0, &mut changed);
                Self::slider(ui, "Softness", &mut p.softness, 0.0, 1.0, &mut changed);
                if sel == 2 {
                    ui.label("Facets");
                    if ui.add(egui::Slider::new(&mut p.facets, 3..=12)).changed() {
                        changed = true;
                    }
                    ui.end_row();
                }
                if sel == 4 {
                    ui.label("Double prism");
                    let mut dbl = p.facets >= 4;
                    if ui.checkbox(&mut dbl, "").changed() {
                        p.facets = if dbl { 6 } else { 3 };
                        changed = true;
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Light").color(ACCENT));
            egui::Grid::new("light").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                Self::slider(ui, "Rainbow split", &mut p.dispersion, 0.0, 1.0, &mut changed);
                Self::slider(ui, "Edge light", &mut p.edge_light, 0.0, 1.0, &mut changed);
                Self::slider(ui, "Edge softness", &mut p.edge_soft, 0.0, 1.0, &mut changed);
                Self::slider(ui, "Bevel", &mut p.bevel, 0.0, 1.0, &mut changed);
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Position").color(ACCENT));
            ui.label(egui::RichText::new("Tip: drag the crosshair on the picture to move the centre.").size(12.0).color(DIM));
            egui::Grid::new("pos").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                Self::slider(ui, "Rotation", &mut p.rotation, -180.0, 180.0, &mut changed);
                Self::slider(ui, "Centre X", &mut p.cx, 0.0, 1.0, &mut changed);
                Self::slider(ui, "Centre Y", &mut p.cy, 0.0, 1.0, &mut changed);
                if sel == 0 || sel == 2 || sel == 3 {
                    Self::slider(ui, "Variation", &mut p.variation, 0.0, 10.0, &mut changed);
                }
                Self::slider(ui, "Mix", &mut p.mix, 0.0, 1.0, &mut changed);
            });
            ui.add_space(4.0);
            if ui.button("Reset this look").clicked() {
                *p = Params::preset(sel);
                changed = true;
            }
        }
        if changed {
            self.dirty = true;
        }

        ui.add_space(14.0);
        ui.separator();
        ui.label(egui::RichText::new("DAVINCI RESOLVE").size(12.0).color(DIM).strong());
        ui.label(
            egui::RichText::new("Installs Glass Lens as a real effect: the Edit page Effects panel (OpenFX) and a Color page node.")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(4.0);
        let (ok, msg) = self.installed;
        ui.label(
            egui::RichText::new(format!("{}  {msg}", if ok { "●" } else { "○" }))
                .size(12.5)
                .color(if ok { Color32::from_rgb(0x7c, 0xc8, 0x7a) } else { ACCENT }),
        );
        let install = egui::Button::new(egui::RichText::new("Install into DaVinci Resolve").color(Color32::BLACK).strong())
            .fill(ACCENT)
            .min_size(Vec2::new(250.0, 32.0));
        if ui.add(install).clicked() {
            let rep = resolve::install(&self.looks);
            self.status = rep.lines;
            self.installed = resolve::install_state();
        }
        if ui.add(egui::Button::new("Save the plug-in files to a folder…").min_size(Vec2::new(250.0, 26.0))).clicked() {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                let dir = dir.join("Glass Lens for Resolve");
                self.status = match resolve::write_bundle(&dir, &self.looks) {
                    Ok(f) => vec![format!("Saved {} files to {} (see README.txt inside).", f.len(), dir.display())],
                    Err(e) => vec![format!("Couldn't save: {e}")],
                };
            }
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "Edit page: Effects > OpenFX > Filters > Glass Lens, drag onto a clip.\nColor page: OpenFX > Glass Lens, drag onto a node.\nPick the Look in the Inspector; each look starts from the same preset as here.",
            )
            .size(12.0)
            .color(DIM),
        );
    }

    fn viewer(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let area = avail.shrink(18.0);
        let (iw, ih) = (self.preview_src.w as f32, self.preview_src.h as f32);
        let s = (area.width() / iw).min(area.height() / ih);
        let size = Vec2::new(iw * s, ih * s);
        let rect = Rect::from_center_size(area.center(), size);

        // Drag (left mouse held) anywhere on the picture to move the glass centre.
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let held = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
        if held || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let nx = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                let ny = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
                let lp = &mut self.looks[self.sel];
                if (lp.cx - nx).abs() > 1e-4 || (lp.cy - ny).abs() > 1e-4 {
                    lp.cx = nx;
                    lp.cy = ny;
                    self.dirty = true;
                }
            }
        }
        let dragging = held;

        let p = ui.painter();
        p.rect_filled(rect.expand(8.0), 10.0, FRAME);
        let tex = if self.compare { &self.orig_tex } else { &self.view_tex };
        if let Some(t) = tex {
            p.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        // the crosshair at the glass centre
        if !self.compare {
            let lp = &self.looks[self.sel];
            let c = Pos2::new(rect.min.x + lp.cx * rect.width(), rect.min.y + lp.cy * rect.height());
            let col = if dragging { ACCENT } else { Color32::from_rgba_unmultiplied(255, 255, 255, 220) };
            let shadow = egui::Stroke::new(3.0f32, Color32::from_black_alpha(120));
            let line = egui::Stroke::new(1.5f32, col);
            let (arm, gap) = (22.0f32, 6.0f32);
            let segs = [
                [Pos2::new(c.x - arm, c.y), Pos2::new(c.x - gap, c.y)],
                [Pos2::new(c.x + gap, c.y), Pos2::new(c.x + arm, c.y)],
                [Pos2::new(c.x, c.y - arm), Pos2::new(c.x, c.y - gap)],
                [Pos2::new(c.x, c.y + gap), Pos2::new(c.x, c.y + arm)],
            ];
            for s in segs {
                p.line_segment(s, shadow);
            }
            for s in segs {
                p.line_segment(s, line);
            }
            p.circle_stroke(c, 12.0, shadow);
            p.circle_stroke(c, 12.0, line);
            p.circle_filled(c, 2.0, col);
        }
        if self.show_name {
            let label = if self.compare { "Original" } else { LOOKS[self.sel] };
            let pos = Pos2::new(rect.center().x, rect.max.y - rect.height() * 0.065);
            let fs = (rect.height() * 0.04).clamp(14.0, 30.0);
            p.text(pos + Vec2::new(1.0, 1.5), Align2::CENTER_CENTER, label, FontId::proportional(fs), Color32::from_black_alpha(160));
            p.text(pos, Align2::CENTER_CENTER, label, FontId::proportional(fs), Color32::from_rgb(0xf2, 0xee, 0xe8));
        }
    }
}

impl eframe::App for LabApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // dropped pictures
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.load_picture(p);
        }

        egui::TopBottomPanel::top("top").frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::symmetric(14, 8))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Glass Lens Lab").size(18.0).color(Color32::WHITE).strong());
                ui.label(egui::RichText::new("glass in front of the lens").size(13.0).color(DIM));
                ui.add_space(20.0);
                if ui.button("Open a frame of your footage…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file() {
                        self.load_picture(p);
                    }
                }
                if ui.button("Sample shot").clicked() {
                    self.set_source(scene::sample_scene(1920, 1080), "Sample shot (bar at night)".into());
                }
                if ui.button("Save this frame…").clicked() {
                    self.save_picture();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_name, "Show look name");
                    let b = ui.selectable_label(self.compare, "  Before / After  ");
                    if b.clicked() {
                        self.compare = !self.compare;
                    }
                    ui.label(egui::RichText::new(&self.source_name).color(DIM));
                });
            });
        });

        egui::TopBottomPanel::bottom("status").frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::symmetric(14, 6))).show(ctx, |ui| {
            for l in &self.status {
                ui.label(egui::RichText::new(l).size(12.5).color(DIM));
            }
        });

        egui::SidePanel::left("looks")
            .exact_width(THUMB_W as f32 + 28.0)
            .resizable(false)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(14, 4)))
            .show(ctx, |ui| self.left_panel(ui));

        egui::SidePanel::right("inspector")
            .exact_width(300.0)
            .resizable(false)
            .frame(egui::Frame::default().fill(PANEL).inner_margin(egui::Margin::symmetric(14, 4)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.right_panel(ui));
            });

        egui::CentralPanel::default().frame(egui::Frame::default().fill(BG)).show(ctx, |ui| self.viewer(ui));

        self.refresh(ctx);

        // --screenshot: draw ~120 frames, save the window picture, quit
        if let Some(path) = self.shot.clone() {
            self.frame += 1;
            ctx.request_repaint();
            if self.frame >= 120 && !self.shot_sent {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.shot_sent = true;
            }
            let shots: Vec<std::sync::Arc<egui::ColorImage>> = ctx.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
                    .collect()
            });
            if let Some(img) = shots.into_iter().next() {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
                if let Err(e) = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                    eprintln!("screenshot failed: {e}");
                }
                std::process::exit(0);
            }
            if self.frame > 600 {
                eprintln!("screenshot never arrived");
                std::process::exit(1);
            }
        }
    }
}
