//! exiftool-rs GUI — metadata viewer/editor with egui
//!
//! Build: cargo build --release --features gui --bin exiftool-rs-gui
//! Run:   ./target/release/exiftool-rs-gui [FILE_OR_DIR]

#[cfg(feature = "gui")]
fn main() {
    use eframe::egui;

    let args: Vec<String> = std::env::args().collect();
    let initial_path = args.get(1).cloned();

    // Load window icon from embedded PNG
    let icon_data = include_bytes!("../assets/icon.png");
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
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([600.0, 400.0])
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
        Box::new(move |_cc| Ok(Box::new(App::new(initial_path)))),
    );
}

#[cfg(not(feature = "gui"))]
fn main() {
    eprintln!("GUI not available. Build with: cargo build --release --features gui --bin exiftool-rs-gui");
    std::process::exit(1);
}

#[cfg(feature = "gui")]
use eframe::egui;

#[cfg(feature = "gui")]
use exiftool_rs::{ExifTool, Tag};

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
    /// Pending edits: (tag_name, new_value)
    pending_edits: Vec<(String, String)>,
    /// Edit popup state
    editing: Option<EditState>,
    /// Current language
    lang: String,
    /// Available languages
    languages: Vec<(&'static str, &'static str)>,
    /// Translations for current language
    translations: Option<std::collections::HashMap<&'static str, &'static str>>,
    /// Status message
    status: String,
    /// Thumbnail texture
    thumbnail: Option<egui::TextureHandle>,
}

#[cfg(feature = "gui")]
struct EditState {
    tag_name: String,
    original_value: String,
    new_value: String,
}

#[cfg(feature = "gui")]
impl App {
    fn new(initial_path: Option<String>) -> Self {
        let mut app = Self {
            et: ExifTool::new(),
            files: Vec::new(),
            current: 0,
            tags: Vec::new(),
            groups: Vec::new(),
            collapsed: std::collections::HashSet::new(),
            icon_texture: None,
            pending_edits: Vec::new(),
            editing: None,
            lang: exiftool_rs::i18n::detect_system_language(),
            languages: exiftool_rs::i18n::AVAILABLE_LANGUAGES.to_vec(),
            translations: None,
            status: "Drop a file or folder to start".into(),
            thumbnail: None,
        };

        // Load translations for detected system language
        if app.lang != "en" {
            app.translations = exiftool_rs::i18n::get_translations(&app.lang);
        }

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
                    matches!(ext_lower.as_str(),
                        "jpg" | "jpeg" | "tif" | "tiff" | "png" | "gif" | "bmp" | "webp" |
                        "cr2" | "cr3" | "crw" | "nef" | "dng" | "arw" | "orf" | "raf" |
                        "rw2" | "pef" | "x3f" | "iiq" | "mrw" | "sr2" | "srf" |
                        "mp4" | "mov" | "avi" | "mkv" | "mts" | "m2ts" |
                        "mp3" | "flac" | "wav" | "ogg" | "aac" |
                        "pdf" | "psd" | "heif" | "heic" | "avif" |
                        "xmp" | "mie" | "exv"
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
            self.status = "No supported files found in folder".into();
        }
    }

    fn load_current(&mut self) {
        if self.current >= self.files.len() { return; }
        let path = self.files[self.current].clone();
        self.pending_edits.clear();
        self.editing = None;
        self.thumbnail = None;

        match self.et.extract_info(path.to_str().unwrap_or("")) {
            Ok(tags) => {
                self.tags = tags;
                self.build_groups();
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let count = self.tags.len();
                self.status = format!("{} tags | {}", count, name);
            }
            Err(e) => {
                self.tags.clear();
                self.groups.clear();
                self.status = format!("Error: {}", e);
            }
        }
    }

    fn build_groups(&mut self) {
        let mut groups: Vec<(String, Vec<Tag>)> = Vec::new();
        let mut current_group = String::new();

        for tag in &self.tags {
            let grp = &tag.group.family0;
            if grp != &current_group {
                current_group = grp.clone();
                groups.push((grp.clone(), Vec::new()));
            }
            if let Some(last) = groups.last_mut() {
                last.1.push(tag.clone());
            }
        }
        self.groups = groups;
    }

    fn navigate(&mut self, delta: isize) {
        if self.files.is_empty() { return; }
        let new = self.current as isize + delta;
        if new >= 0 && (new as usize) < self.files.len() {
            self.current = new as usize;
            self.load_current();
        }
    }

    fn set_language(&mut self, lang: &str) {
        self.lang = lang.to_string();
        self.translations = if lang == "en" {
            None
        } else {
            exiftool_rs::i18n::get_translations(lang)
        };
    }

    fn translate<'a>(&'a self, tag_name: &str, description: &'a str) -> &'a str {
        if let Some(ref tr) = self.translations {
            if let Some(translated) = tr.get(tag_name) {
                return translated;
            }
        }
        description
    }

    fn is_writable(tag_name: &str) -> bool {
        // Composites and File-level tags are read-only
        !matches!(tag_name,
            "FileSize" | "FileName" | "Directory" | "FileType" | "MIMEType" |
            "FileModifyDate" | "FileAccessDate" | "FileInodeChangeDate" |
            "FilePermissions" | "FileTypeExtension" | "ExifToolVersion" |
            "ImageSize" | "Megapixels" | "ShutterSpeed" | "Aperture" |
            "LightValue" | "FocalLength35efl" | "CircleOfConfusion" |
            "DOF" | "FOV" | "HyperfocalDistance" | "ScaleFactor35efl" |
            "LensID" | "Lens" | "Lens35efl" | "DriveMode" | "ShootingMode" |
            "BlueBalance" | "RedBalance" | "WB_RGGBLevels" | "GPSPosition"
        )
    }
}

#[cfg(feature = "gui")]
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle dropped files
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                if let Some(path) = i.raw.dropped_files[0].path.as_ref() {
                    if path.is_dir() {
                        self.open_folder(path);
                    } else {
                        // Open the file's parent folder and navigate to it
                        if let Some(parent) = path.parent() {
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
                }
            }
        });

        // Keyboard navigation
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowLeft) { self.navigate(-1); }
            if i.key_pressed(egui::Key::ArrowRight) { self.navigate(1); }
            if i.key_pressed(egui::Key::Home) {
                self.current = 0;
                self.load_current();
            }
            if i.key_pressed(egui::Key::End) {
                if !self.files.is_empty() {
                    self.current = self.files.len() - 1;
                    self.load_current();
                }
            }
        });

        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Open").clicked() {
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
                if ui.button("📁 Folder").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.open_folder(&path);
                    }
                }
                if ui.button("📋 Copy").clicked() {
                    let text: String = self.tags.iter()
                        .map(|t| format!("{}: {}", t.name, t.print_value))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ctx.copy_text(text);
                }
                if ui.button("💾 Save").clicked() {
                    if !self.pending_edits.is_empty() {
                        if let Some(path) = self.files.get(self.current) {
                            let path_str = path.to_string_lossy().to_string();
                            let mut et = ExifTool::new();
                            for (tag, value) in &self.pending_edits {
                                et.set_new_value(tag, Some(value));
                            }
                            match et.write_info(&path_str, &path_str) {
                                Ok(_) => {
                                    self.status = format!("Saved {} edits", self.pending_edits.len());
                                    self.pending_edits.clear();
                                    self.load_current();
                                }
                                Err(e) => {
                                    self.status = format!("Save error: {}", e);
                                }
                            }
                        }
                    }
                }

                ui.separator();

                // Language selector
                let langs: Vec<(&str, &str)> = self.languages.clone();
                let current_lang = self.lang.clone();
                let current_lang_name = langs.iter()
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
            });
        });

        // Navigation bar
        if !self.files.is_empty() {
            egui::TopBottomPanel::top("navigation").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 150.0);

                    let can_prev = self.current > 0;
                    let can_next = self.current + 1 < self.files.len();

                    if ui.add_enabled(can_prev, egui::Button::new("◀")).clicked() {
                        self.navigate(-1);
                    }

                    let filename = self.files.get(self.current)
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    ui.label(egui::RichText::new(
                        format!("  {}  ({}/{})  ", filename, self.current + 1, self.files.len())
                    ).strong());

                    if ui.add_enabled(can_next, egui::Button::new("▶")).clicked() {
                        self.navigate(1);
                    }
                });
            });
        }

        // Status bar
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if !self.pending_edits.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(
                        format!("{} modification(s) en attente", self.pending_edits.len())
                    ).color(egui::Color32::YELLOW));
                }
            });
        });

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.tags.is_empty() && self.files.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 4.0);

                    // Show app icon
                    if self.icon_texture.is_none() {
                        let icon_data = include_bytes!("../assets/icon.png");
                        if let Ok(img) = image::load_from_memory(icon_data) {
                            let rgba = img.to_rgba8();
                            let (w, h) = rgba.dimensions();
                            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &rgba.into_raw(),
                            );
                            self.icon_texture = Some(ctx.load_texture("icon", color_image, egui::TextureOptions::LINEAR));
                        }
                    }
                    if let Some(ref tex) = self.icon_texture {
                        ui.image((tex.id(), egui::vec2(128.0, 128.0)));
                    }

                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("exiftool-rs")
                        .size(28.0)
                        .strong());
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Drop a file or folder here\nor use Open / Folder buttons")
                        .size(16.0)
                        .color(egui::Color32::GRAY));
                });
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                let groups = self.groups.clone();
                for (group_name, group_tags) in &groups {
                    let is_collapsed = self.collapsed.contains(group_name);
                    let header = if is_collapsed {
                        format!("▶ {} ({})", group_name, group_tags.len())
                    } else {
                        format!("▼ {} ({})", group_name, group_tags.len())
                    };

                    if ui.selectable_label(false, egui::RichText::new(&header).strong().size(14.0)).clicked() {
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

                                    // Check if this tag has a pending edit
                                    let display_value = self.pending_edits.iter()
                                        .find(|(name, _)| name == &tag.name)
                                        .map(|(_, v)| v.as_str())
                                        .unwrap_or(&tag.print_value);

                                    let is_edited = self.pending_edits.iter().any(|(n, _)| n == &tag.name);

                                    // Tag name (description)
                                    ui.label(egui::RichText::new(desc).color(egui::Color32::LIGHT_BLUE));

                                    // Value — double-click to edit
                                    let value_text = if is_edited {
                                        egui::RichText::new(display_value).color(egui::Color32::YELLOW)
                                    } else {
                                        egui::RichText::new(display_value)
                                    };

                                    let response = ui.label(value_text);

                                    if response.double_clicked() && Self::is_writable(&tag.name) {
                                        self.editing = Some(EditState {
                                            tag_name: tag.name.clone(),
                                            original_value: tag.print_value.clone(),
                                            new_value: tag.print_value.clone(),
                                        });
                                    }

                                    // Show "read-only" cursor for non-writable tags
                                    if response.hovered() && !Self::is_writable(&tag.name) {
                                        response.on_hover_text("Read-only (composite/computed tag)");
                                    }

                                    ui.end_row();
                                }
                            });
                        ui.add_space(8.0);
                    }
                }
            });
        });

        // Edit popup window
        let mut close_edit = false;
        if let Some(ref mut edit) = self.editing {
            let mut open = true;
            egui::Window::new(format!("Edit: {}", edit.tag_name))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Original: {}", edit.original_value));
                    ui.add_space(4.0);
                    let response = ui.text_edit_singleline(&mut edit.new_value);

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            if edit.new_value != edit.original_value {
                                // Remove any existing edit for this tag
                                self.pending_edits.retain(|(n, _)| n != &edit.tag_name);
                                self.pending_edits.push((edit.tag_name.clone(), edit.new_value.clone()));
                            }
                            close_edit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_edit = true;
                        }
                    });
                });
            if !open { close_edit = true; }
        }
        if close_edit {
            self.editing = None;
        }
    }
}
