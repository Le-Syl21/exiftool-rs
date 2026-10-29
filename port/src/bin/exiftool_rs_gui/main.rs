//! exiftool-rs GUI — metadata viewer/editor with egui, on the port: the
//! root crate's GUI (src/gui.rs) over the API facade, so what it shows is
//! ExifTool's own, tag descriptions and values in the chosen language
//! included (ExifTool's `Lang` option).
//!
//! Build: cargo build --release --features gui --bin exiftool-rs-gui
//! Run:   ./target/release/exiftool-rs-gui [-lang LANG] [FILE_OR_DIR]

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
mod i18n;

#[cfg(feature = "gui")]
fn main() {
    use eframe::egui;

    let args: Vec<String> = std::env::args().collect();
    let mut initial_path: Option<String> = None;
    let mut forced_lang: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-lang" => {
                if i + 1 < args.len() {
                    i += 1;
                    forced_lang = Some(args[i].clone());
                }
            }
            arg if !arg.starts_with('-') => {
                initial_path = Some(arg.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    // Load window icon from embedded PNG
    let icon_data = include_bytes!("../../../../assets/icon.png");
    let icon = image::load_from_memory(icon_data)
        .map(|img| {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        })
        .ok();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([600.0, 700.0])
        .with_min_inner_size([400.0, 400.0])
        .with_drag_and_drop(true);

    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let _ = eframe::run_native(
        "exiftool-rs",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(initial_path, forced_lang)))),
    );
}

#[cfg(not(feature = "gui"))]
fn main() {
    eprintln!(
        "GUI not available. Build with: cargo build --release --features gui --bin exiftool-rs-gui"
    );
    std::process::exit(1);
}

#[cfg(feature = "gui")]
use eframe::egui;

#[cfg(feature = "gui")]
use exiftool_port::{ExifTool, Options, Tag};

#[cfg(feature = "gui")]
struct App {
    et: ExifTool,
    /// All files in current folder
    files: Vec<std::path::PathBuf>,
    /// Current file index
    current: usize,
    /// Extracted tags for current file
    tags: Vec<Tag>,
    /// Grouped tags (group_name, tags)
    groups: Vec<(String, Vec<Tag>)>,
    /// Collapsed groups
    collapsed: std::collections::HashSet<String>,
    /// App icon texture for welcome screen
    icon_texture: Option<egui::TextureHandle>,
    /// Show About dialog
    show_about: bool,
    /// Pending edits: (tag_name, new_value)
    pending_edits: Vec<(String, String)>,
    /// Edit popup state
    editing: Option<EditState>,
    /// Current language
    lang: String,
    /// Available languages
    languages: Vec<(&'static str, &'static str)>,
    /// Status message
    status: String,
    /// Thumbnail texture
    thumbnail: Option<egui::TextureHandle>,
    /// Whether fonts have been configured
    fonts_configured: bool,
    /// Writable tags for current file type (None = any tag writable)
    writable_tags: Option<Option<std::collections::HashSet<&'static str>>>,
    /// Whether window needs auto-resize after loading tags
    needs_resize: bool,
    /// Flash feedback timer (for copy/save button visual confirmation)
    flash_until: Option<std::time::Instant>,
}

#[cfg(feature = "gui")]
struct EditState {
    tag_name: String,
    original_value: String,
    new_value: String,
}

#[cfg(feature = "gui")]
impl App {
    fn new(initial_path: Option<String>, forced_lang: Option<String>) -> Self {
        let lang = forced_lang.unwrap_or_else(i18n::detect_system_language);
        let mut app = Self {
            // tag descriptions and values in the language, by ExifTool
            et: ExifTool::with_options(Self::options(&lang)),
            files: Vec::new(),
            current: 0,
            tags: Vec::new(),
            groups: Vec::new(),
            collapsed: std::collections::HashSet::new(),
            icon_texture: None,
            show_about: false,
            pending_edits: Vec::new(),
            editing: None,
            lang,
            languages: i18n::AVAILABLE_LANGUAGES.to_vec(),
            status: String::new(),
            thumbnail: None,
            fonts_configured: false,
            writable_tags: None,
            needs_resize: false,
            flash_until: None,
        };

        app.status = i18n::ui_text(&app.lang, "drop_start").to_string();

        if let Some(path) = initial_path {
            let p = std::path::Path::new(&path);
            if p.is_dir() {
                app.open_folder(p);
            } else if p.is_file() {
                app.open_file(p);
            }
        }

        app
    }

    fn open_file(&mut self, path: &std::path::Path) {
        self.files = vec![path.to_path_buf()];
        self.current = 0;
        self.load_current();
    }

    fn open_folder(&mut self, path: &std::path::Path) {
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                // Filter by supported extensions
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    matches!(
                        ext_lower.as_str(),
                        "jpg"
                            | "jpeg"
                            | "tif"
                            | "tiff"
                            | "png"
                            | "gif"
                            | "bmp"
                            | "webp"
                            | "cr2"
                            | "cr3"
                            | "crw"
                            | "nef"
                            | "dng"
                            | "arw"
                            | "orf"
                            | "raf"
                            | "rw2"
                            | "pef"
                            | "x3f"
                            | "iiq"
                            | "mrw"
                            | "sr2"
                            | "srf"
                            | "mp4"
                            | "mov"
                            | "avi"
                            | "mkv"
                            | "mts"
                            | "m2ts"
                            | "mp3"
                            | "flac"
                            | "wav"
                            | "ogg"
                            | "aac"
                            | "pdf"
                            | "psd"
                            | "heif"
                            | "heic"
                            | "avif"
                            | "xmp"
                            | "mie"
                            | "exv"
                    )
                } else {
                    false
                }
            })
            .collect();
        files.sort();
        self.files = files;
        self.current = 0;
        if !self.files.is_empty() {
            self.load_current();
        } else {
            self.status = i18n::ui_text(&self.lang, "no_files").to_string();
        }
    }

    fn load_current(&mut self) {
        if self.current >= self.files.len() {
            return;
        }
        let path = self.files[self.current].clone();
        self.pending_edits.clear();
        self.editing = None;
        self.thumbnail = None;

        // Detect file type for writable tags
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.writable_tags = if let Some(ft) = exiftool_port::file_type::detect_from_extension(ext) {
            Some(ExifTool::writable_tags(ft))
        } else {
            Some(Some(std::collections::HashSet::new()))
        };

        self.read_tags();
    }

    /// Read the current file's tags, in the current language.
    fn read_tags(&mut self) {
        let Some(path) = self.files.get(self.current).cloned() else {
            return;
        };
        match self.et.extract_info(path.to_str().unwrap_or("")) {
            Ok(tags) => {
                self.tags = tags;
                self.build_groups();
                self.needs_resize = true;
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let count = self.tags.len();
                self.status = format!("{} tags | {}", count, name);
            }
            Err(e) => {
                self.tags.clear();
                self.groups.clear();
                self.status = format!("{}: {}", i18n::ui_text(&self.lang, "error"), e);
            }
        }
    }

    /// One section per family-0 group, in order of first appearance. ExifTool
    /// lists tags in file order, so a group's tags need not be consecutive
    /// (EXIF, MakerNotes, EXIF again...).
    fn build_groups(&mut self) {
        let mut groups: Vec<(String, Vec<Tag>)> = Vec::new();
        for tag in &self.tags {
            let grp = &tag.group.family0;
            match groups.iter_mut().find(|(g, _)| g == grp) {
                Some((_, tags)) => tags.push(tag.clone()),
                None => groups.push((grp.clone(), vec![tag.clone()])),
            }
        }
        self.groups = groups;
    }

    fn navigate(&mut self, delta: isize) {
        if self.files.is_empty() {
            return;
        }
        let new = self.current as isize + delta;
        if new >= 0 && (new as usize) < self.files.len() {
            self.current = new as usize;
            self.load_current();
        }
    }

    /// The facade's options for a GUI language: ExifTool's `Lang`.
    fn options(lang: &str) -> Options {
        Options {
            lang: i18n::exiftool_lang(lang),
            ..Options::default()
        }
    }

    fn set_language(&mut self, lang: &str) {
        self.lang = lang.to_string();
        // ExifTool translates as it reads: read the file again in the new
        // language (pending edits are kept, as before)
        self.et = ExifTool::with_options(Self::options(lang));
        self.read_tags();
    }

    /// The tag's description: ExifTool's `GetDescription`, already in the
    /// current language (the ExifTool reading the file has its `Lang`).
    fn translate<'a>(&'a self, _tag_name: &str, description: &'a str) -> &'a str {
        description
    }

    /// The tag's printed value: ExifTool's PrintConv, already localized
    /// as ExifTool localizes it (in a standalone XMP file, only the XMP and
    /// Composite values, as ExifTool reads the rest without PrintConv).
    fn translate_value(&self, _group0: &str, _group1: &str, _tag_name: &str, value: &str) -> String {
        value.to_string()
    }

    fn is_tag_writable(&self, tag_name: &str) -> bool {
        match &self.writable_tags {
            None => false,      // no file loaded
            Some(None) => true, // open-ended format (PNG, FLAC, MKV...)
            Some(Some(set)) => set.contains(tag_name.to_lowercase().as_str()),
        }
    }
}

#[cfg(feature = "gui")]
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // egui 0.35: App::update(ctx) became App::ui(ui). Keep a cheap Arc-clone
        // of the context so the existing ctx-based calls below stay unchanged.
        let ctx = ui.ctx().clone();

        // Configure fonts for CJK, Arabic, Devanagari, Bengali support
        if !self.fonts_configured {
            self.fonts_configured = true;
            let mut fonts = egui::FontDefinitions::default();

            // Load Noto fonts from noto-fonts-dl (downloaded at build time, XZ-compressed)
            for (name, data) in noto_fonts_dl::load_fonts() {
                fonts.font_data.insert(
                    name.clone(),
                    std::sync::Arc::new(egui::FontData::from_owned(data.clone())),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push(name.clone());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push(name.clone());
            }

            ctx.set_fonts(fonts);
        }

        // Handle dropped files
        ctx.input(|i| {
            if let Some(dropped) = i.raw.dropped_files.first() {
                // egui 0.36 made `DroppedFile` a trait whose `path()` returns
                // `&Path` rather than the old `Option<PathBuf>` field: a
                // dropped file now always has one.
                let path = dropped.path();
                if path.is_dir() {
                    self.open_folder(path);
                } else if let Some(parent) = path.parent() {
                    // Open the file's parent folder and navigate to it
                    self.open_folder(parent);
                    // Find the file in the list
                    if let Some(idx) = self.files.iter().position(|f| f == path) {
                        self.current = idx;
                        self.load_current();
                    }
                } else {
                    self.open_file(path);
                }
            }
        });

        // Keyboard navigation
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.navigate(-1);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                self.navigate(1);
            }
            if i.key_pressed(egui::Key::Home) {
                self.current = 0;
                self.load_current();
            }
            if i.key_pressed(egui::Key::End) && !self.files.is_empty() {
                self.current = self.files.len() - 1;
                self.load_current();
            }
        });

        // Top toolbar
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(i18n::ui_text(&self.lang, "open"))
                    .on_hover_text(i18n::ui_text(&self.lang, "tooltip_open"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        if let Some(parent) = path.parent() {
                            self.open_folder(parent);
                            if let Some(idx) = self.files.iter().position(|f| f == &path) {
                                self.current = idx;
                                self.load_current();
                            }
                        }
                    }
                }
                if ui
                    .button(i18n::ui_text(&self.lang, "folder"))
                    .on_hover_text(i18n::ui_text(&self.lang, "tooltip_folder"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.open_folder(&path);
                    }
                }
                // Show flash feedback on copy/save buttons
                let is_flashing = self
                    .flash_until
                    .is_some_and(|t| t > std::time::Instant::now());
                if is_flashing {
                    ctx.request_repaint();
                }

                if ui
                    .button(i18n::ui_text(&self.lang, "copy"))
                    .on_hover_text(i18n::ui_text(&self.lang, "tooltip_copy"))
                    .clicked()
                {
                    let text: String = self
                        .tags
                        .iter()
                        .map(|t| format!("{}: {}", t.name, t.print_value))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ctx.copy_text(text);
                    self.status = i18n::ui_text(&self.lang, "copied").to_string();
                    self.flash_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(800));
                }
                if ui
                    .button(i18n::ui_text(&self.lang, "save"))
                    .on_hover_text(i18n::ui_text(&self.lang, "tooltip_save"))
                    .clicked()
                    && !self.pending_edits.is_empty()
                {
                    if let Some(path) = self.files.get(self.current) {
                        let path_str = path.to_string_lossy().to_string();
                        // (in the current language: ExifTool takes the
                        // values back in the language it shows them in)
                        let mut et = ExifTool::with_options(Self::options(&self.lang));
                        for (tag, value) in &self.pending_edits {
                            et.set_new_value(tag, Some(value));
                        }
                        match et.write_info(&path_str, &path_str) {
                            Ok(_) => {
                                self.status = format!(
                                    "{} {}",
                                    self.pending_edits.len(),
                                    i18n::ui_text(&self.lang, "saved")
                                );
                                self.pending_edits.clear();
                                self.load_current();
                                self.flash_until = Some(
                                    std::time::Instant::now()
                                        + std::time::Duration::from_millis(800),
                                );
                            }
                            Err(e) => {
                                self.status = format!(
                                    "{}: {}",
                                    i18n::ui_text(&self.lang, "save_error"),
                                    e
                                );
                            }
                        }
                    }
                }

                ui.separator();

                // Language selector
                let langs: Vec<(&str, &str)> = self.languages.clone();
                let current_lang = self.lang.clone();
                let current_lang_name = langs
                    .iter()
                    .find(|(c, _)| *c == current_lang)
                    .map(|(_, n)| *n)
                    .unwrap_or("English");
                let mut new_lang: Option<String> = None;
                egui::ComboBox::from_label("")
                    .selected_text(format!("🌐 {}", current_lang_name))
                    .show_ui(ui, |ui| {
                        for (code, name) in &langs {
                            if ui.selectable_label(current_lang == *code, *name).clicked() {
                                new_lang = Some(code.to_string());
                            }
                        }
                    });
                if let Some(l) = new_lang {
                    self.set_language(&l);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(i18n::ui_text(&self.lang, "about"))
                        .clicked()
                    {
                        self.show_about = true;
                    }
                });
            });
        });

        // Navigation bar
        if !self.files.is_empty() {
            egui::Panel::top("navigation").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 150.0);

                    let can_prev = self.current > 0;
                    let can_next = self.current + 1 < self.files.len();

                    if ui.add_enabled(can_prev, egui::Button::new("◀")).clicked() {
                        self.navigate(-1);
                    }

                    let filename = self
                        .files
                        .get(self.current)
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    ui.label(
                        egui::RichText::new(format!(
                            "  {}  ({}/{})  ",
                            filename,
                            self.current + 1,
                            self.files.len()
                        ))
                        .strong(),
                    );

                    if ui.add_enabled(can_next, egui::Button::new("▶")).clicked() {
                        self.navigate(1);
                    }
                });
            });

            // Editable hint bar
            egui::Panel::top("editable_hint").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(i18n::ui_text(
                            &self.lang,
                            "editable_hint",
                        ))
                        .color(egui::Color32::from_rgb(100, 200, 100)),
                    );
                });
            });
        }

        // Status bar
        let is_flashing = self
            .flash_until
            .is_some_and(|t| t > std::time::Instant::now());
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if is_flashing {
                    ui.label(
                        egui::RichText::new(&self.status)
                            .color(egui::Color32::GREEN)
                            .strong(),
                    );
                } else {
                    ui.label(&self.status);
                }
                if !self.pending_edits.is_empty() {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            self.pending_edits.len(),
                            i18n::ui_text(&self.lang, "pending")
                        ))
                        .color(egui::Color32::YELLOW),
                    );
                }
            });
        });

        // Load icon texture once
        if self.icon_texture.is_none() {
            let icon_data = include_bytes!("../../../../assets/icon.png");
            if let Ok(img) = image::load_from_memory(icon_data) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    &rgba.into_raw(),
                );
                self.icon_texture =
                    Some(ctx.load_texture("icon", color_image, egui::TextureOptions::LINEAR));
            }
        }

        // Main panel: tags
        egui::CentralPanel::default().show(ui, |ui| {
            // Draw watermark logo in background
            if let Some(ref tex) = self.icon_texture {
                let panel_rect = ui.available_rect_before_wrap();
                let logo_size = panel_rect.height().min(panel_rect.width()) * 0.5;
                let center = panel_rect.center();
                let logo_rect =
                    egui::Rect::from_center_size(center, egui::vec2(logo_size, logo_size));
                ui.painter().image(
                    tex.id(),
                    logo_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::from_white_alpha(128),
                );
            }

            if self.tags.is_empty() && self.files.is_empty() {
                return;
            }

            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let groups = self.groups.clone();
                    for (group_name, group_tags) in &groups {
                        let is_collapsed = self.collapsed.contains(group_name);
                        let header = if is_collapsed {
                            format!("▶ {} ({})", group_name, group_tags.len())
                        } else {
                            format!("▼ {} ({})", group_name, group_tags.len())
                        };

                        if ui
                            .selectable_label(
                                false,
                                egui::RichText::new(&header).strong().size(14.0),
                            )
                            .clicked()
                        {
                            if is_collapsed {
                                self.collapsed.remove(group_name);
                            } else {
                                self.collapsed.insert(group_name.clone());
                            }
                        }

                        if !is_collapsed {
                            egui::Grid::new(format!("grid_{}", group_name))
                                .num_columns(2)
                                .spacing([20.0, 4.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for tag in group_tags {
                                        let desc = self.translate(&tag.name, &tag.description);

                                        // Check if this tag has a pending edit; otherwise show
                                        // the (optionally localized) print value.
                                        let display_value = self
                                            .pending_edits
                                            .iter()
                                            .find(|(name, _)| name == &tag.name)
                                            .map(|(_, v)| v.clone())
                                            .unwrap_or_else(|| {
                                                self.translate_value(
                                                    &tag.group.family0,
                                                    &tag.group.family1,
                                                    &tag.name,
                                                    &tag.print_value,
                                                )
                                            });

                                        let is_edited =
                                            self.pending_edits.iter().any(|(n, _)| n == &tag.name);

                                        // Tag name : value (with separator)
                                        ui.label(
                                            egui::RichText::new(format!("{} :", desc))
                                                .color(egui::Color32::LIGHT_BLUE)
                                                .strong(),
                                        );

                                        // Value — double-click to edit
                                        let writable = self.is_tag_writable(&tag.name);
                                        let value_text = if is_edited {
                                            egui::RichText::new(display_value.as_str())
                                                .color(egui::Color32::YELLOW)
                                                .strong()
                                        } else if writable {
                                            egui::RichText::new(display_value.as_str())
                                                .color(egui::Color32::from_rgb(100, 200, 100))
                                                .strong()
                                        } else {
                                            egui::RichText::new(display_value.as_str())
                                                .color(egui::Color32::from_rgb(200, 100, 100))
                                                .strong()
                                        };

                                        let response = ui.label(value_text);

                                        if response.double_clicked()
                                            && self.is_tag_writable(&tag.name)
                                        {
                                            self.editing = Some(EditState {
                                                tag_name: tag.name.clone(),
                                                original_value: tag.print_value.clone(),
                                                new_value: tag.print_value.clone(),
                                            });
                                        }

                                        // Show "read-only" cursor for non-writable tags
                                        if response.hovered() && !self.is_tag_writable(&tag.name) {
                                            response.on_hover_text(i18n::ui_text(
                                                &self.lang,
                                                "read_only",
                                            ));
                                        }

                                        ui.end_row();
                                    }
                                });
                            ui.add_space(8.0);
                        }
                    }
                });
            });
        });

        // Edit popup window
        let mut close_edit = false;
        if let Some(ref mut edit) = self.editing {
            let mut open = true;
            egui::Window::new(format!(
                "{}: {}",
                i18n::ui_text(&self.lang, "edit"),
                edit.tag_name
            ))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(&ctx, |ui| {
                ui.label(format!(
                    "{}: {}",
                    i18n::ui_text(&self.lang, "original"),
                    edit.original_value
                ));
                ui.add_space(4.0);
                let response = ui.text_edit_singleline(&mut edit.new_value);

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(i18n::ui_text(&self.lang, "ok"))
                        .clicked()
                        || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        if edit.new_value != edit.original_value {
                            // Remove any existing edit for this tag
                            self.pending_edits.retain(|(n, _)| n != &edit.tag_name);
                            self.pending_edits
                                .push((edit.tag_name.clone(), edit.new_value.clone()));
                        }
                        close_edit = true;
                    }
                    if ui
                        .button(i18n::ui_text(&self.lang, "cancel"))
                        .clicked()
                    {
                        close_edit = true;
                    }
                });
            });
            if !open {
                close_edit = true;
            }
        }
        if close_edit {
            self.editing = None;
        }

        // About dialog
        if self.show_about {
            let mut open = true;
            egui::Window::new("About exiftool-rs")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        if let Some(ref tex) = self.icon_texture {
                            ui.image((tex.id(), egui::vec2(64.0, 64.0)));
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("exiftool-rs").size(20.0).strong());
                        ui.label(format!("v{}", exiftool_port::VERSION));
                        ui.add_space(8.0);
                        ui.label("A pure Rust reimplementation of ExifTool");
                        ui.add_space(8.0);
                        ui.label("This program is free software: you can redistribute it");
                        ui.label("and/or modify it under the terms of the GNU General");
                        ui.label("Public License as published by the Free Software");
                        ui.label("Foundation, either version 3 of the License, or");
                        ui.label("(at your option) any later version.");
                        ui.add_space(4.0);
                        ui.label("This program is distributed WITHOUT ANY WARRANTY.");
                        ui.add_space(4.0);
                        ui.hyperlink_to(
                            "Full license text (GPLv3)",
                            "https://www.gnu.org/licenses/gpl-3.0.html",
                        );
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label("Based on ExifTool by Phil Harvey");
                        ui.hyperlink_to("exiftool.org", "https://exiftool.org/");
                        ui.add_space(4.0);
                        ui.hyperlink_to("GitHub", "https://github.com/Le-Syl21/exiftool-rs");
                        ui.add_space(4.0);
                        ui.hyperlink_to(
                            "💬 Discord — community & support",
                            "https://discord.gg/T37DYHmt2j",
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "Sylvain (project creator) + Claude (implementation)",
                            )
                            .small()
                            .color(egui::Color32::GRAY),
                        );
                    });
                });
            if !open {
                self.show_about = false;
            }
        }
    }
}

// The model with no window: what the panels would show, as App holds it.
#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    const CANON: &str = "../tests/images/Canon.jpg";

    /// The description and the value the panel shows for a tag.
    fn shown(app: &App, name: &str) -> Option<(String, String)> {
        let (_, tags) = app.groups.iter().find(|(_, tags)| tags.iter().any(|t| t.name == name))?;
        let tag = tags.iter().find(|t| t.name == name)?;
        let desc = app.translate(&tag.name, &tag.description).to_string();
        let value = app.translate_value(&tag.group.family0, &tag.group.family1, &tag.name, &tag.print_value);
        Some((desc, value))
    }

    #[test]
    fn a_file_is_shown_in_groups_with_exiftools_english() {
        let app = App::new(Some(CANON.to_string()), Some("en".to_string()));
        assert_eq!(app.files.len(), 1);
        assert!(app.status.ends_with("tags | Canon.jpg"), "{}", app.status);
        let names: Vec<&str> = app.groups.iter().map(|(g, _)| g.as_str()).collect();
        assert!(names.contains(&"EXIF") && names.contains(&"MakerNotes") && names.contains(&"Composite"), "{names:?}");
        // one section per group, though ExifTool lists EXIF and MakerNotes tags interleaved
        let mut unique = names.clone();
        unique.dedup();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "{names:?}");
        assert_eq!(shown(&app, "Make"), Some(("Make".into(), "Canon".into())));
        assert_eq!(shown(&app, "Flash").map(|s| s.1), Some("No Flash".into()));
        // a JPEG's EXIF is writable, a composite or a file property is not
        assert!(app.is_tag_writable("Artist"));
        assert!(!app.is_tag_writable("Megapixels") && !app.is_tag_writable("FileSize"));
    }

    #[test]
    fn descriptions_and_values_are_exiftools_in_the_language() {
        // (as `exiftool -lang fr` prints them)
        let mut app = App::new(Some(CANON.to_string()), Some("fr".to_string()));
        assert_eq!(shown(&app, "Make"), Some(("Fabricant".into(), "Canon".into())));
        assert_eq!(shown(&app, "Flash").map(|s| s.1), Some("Flash non déclenché".into()));
        assert_eq!(shown(&app, "Orientation"), Some(("Orientation de l'image".into(), "Horizontale (normale)".into())));
        // zh is ExifTool's zh_cn
        app.set_language("zh");
        assert_eq!(shown(&app, "Make").map(|s| s.0), Some("厂商".into()));
        assert_eq!(i18n::ui_text(&app.lang, "open"), "打开");
    }

    #[test]
    fn the_added_languages_come_from_their_lang_modules() {
        let mut app = App::new(Some(CANON.to_string()), Some("pt".to_string()));
        assert_eq!(shown(&app, "Make").map(|s| s.0), Some("Fabricante da Câmera".into()));
        assert_eq!(i18n::ui_text(&app.lang, "open"), "Abrir");
        app.set_language("ar");
        assert_eq!(shown(&app, "Model").map(|s| s.0), Some("موديل الكاميرا".into()));
        for lang in ["hi", "bn"] {
            app.set_language(lang);
            let desc = shown(&app, "Make").map(|s| s.0).unwrap();
            assert!(desc != "Make" && !desc.is_ascii(), "{lang}: {desc}");
        }
    }

    #[test]
    fn every_language_of_the_selector_has_its_strings() {
        for (code, _) in i18n::AVAILABLE_LANGUAGES {
            for key in ["open", "folder", "copy", "save", "about", "drop_start", "editable_hint", "read_only", "ok", "cancel"] {
                let text = i18n::ui_text(code, key);
                assert!(!text.is_empty() && text != key, "{code} {key}");
            }
        }
        assert_eq!(i18n::ui_text("de", "no_such_key"), "no_such_key");
        assert_eq!(i18n::exiftool_lang("en"), None);
        assert_eq!(i18n::exiftool_lang("zh_CN").as_deref(), Some("zh"));
    }

    #[test]
    fn an_edit_in_the_language_is_saved() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/scratch/gui_edit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("Canon.jpg");
        std::fs::copy(CANON, &copy).unwrap();
        let path = copy.to_string_lossy().into_owned();
        let app = App::new(Some(path.clone()), Some("fr".to_string()));
        // what the Save button does, with an Orientation typed in French
        let mut et = ExifTool::with_options(App::options(&app.lang));
        et.set_new_value("Orientation", Some("Rotation antihoraire de 90°"));
        et.set_new_value("Artist", Some("Sylvain"));
        assert_eq!(et.write_info(&path, &path).unwrap(), 2);
        let app = App::new(Some(path), Some("en".to_string()));
        assert_eq!(shown(&app, "Orientation").map(|s| s.1), Some("Rotate 90 CW".into()));
        assert_eq!(shown(&app, "Artist").map(|s| s.1), Some("Sylvain".into()));
    }
}
