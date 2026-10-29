//! exiftool-rs GUI — metadata viewer/editor with egui, on the port: the
//! root crate's GUI (src/gui.rs) over the API facade, so what it shows is
//! ExifTool's own, tag descriptions and values in the chosen language
//! included (ExifTool's `Lang` option).
//!
//! Build: cargo build --release --features gui --bin exiftool-rs-gui
//! Run:   ./target/release/exiftool-rs-gui [-lang LANG] [FILE_OR_DIR]
//!
//! Keyboard: Tab / Shift+Tab move between the controls and the values,
//! Enter (or Space) on an editable value edits it, Enter confirms and
//! Escape cancels; Ctrl+O opens a file, Ctrl+S saves; Alt+Left / Alt+Right
//! (or Left / Right, Home / End when nothing has the focus) go through the
//! files of the folder.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// See the rusty_alloc-api entry in Cargo.toml.
#[cfg(feature = "rusty-alloc")]
#[global_allocator]
static GLOBAL: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;

#[cfg(feature = "gui")]
mod i18n;
#[cfg(feature = "gui")]
mod settings;
#[cfg(feature = "gui")]
mod theme;

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
    let icon_data = include_bytes!("../../../assets/icon.png");
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
        .with_title("exiftool-rs")
        .with_inner_size([820.0, 760.0])
        .with_min_inner_size([480.0, 400.0])
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
use egui::{Align, Layout, RichText, Role, WidgetInfo};

#[cfg(feature = "gui")]
use exiftool_rs::{ExifTool, Options, Tag};

#[cfg(feature = "gui")]
use theme::Palette;

/// The mark before an editable value, and before a value edited and not
/// saved yet (the color is not the only cue: color-blind users see the
/// mark, screen readers say the state).
#[cfg(feature = "gui")]
const EDITABLE_MARK: &str = "✏";
#[cfg(feature = "gui")]
const MODIFIED_MARK: &str = "●";

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
    /// Whether fonts and visuals have been configured
    fonts_configured: bool,
    /// Writable tags for current file type (None = any tag writable)
    writable_tags: Option<Option<std::collections::HashSet<&'static str>>>,
    /// Whether window needs auto-resize after loading tags
    needs_resize: bool,
    /// Flash feedback timer (for copy/save button visual confirmation)
    flash_until: Option<std::time::Instant>,
    /// System, light or dark (kept in the settings)
    theme: egui::ThemePreference,
    /// The theme preference last given to egui
    applied_theme: Option<egui::ThemePreference>,
    /// The window title last sent (it names the file, for screen readers too)
    window_title: String,
    /// The value cell to give the keyboard focus back to (after the edit
    /// dialog closes)
    refocus: Option<egui::Id>,
    /// The width of the end of the toolbar (language, theme, About) in the
    /// last frame, to put it at the end with its controls in reading order
    toolbar_end_width: f32,
}

#[cfg(feature = "gui")]
struct EditState {
    tag_name: String,
    description: String,
    original_value: String,
    new_value: String,
    /// The text field has been given the focus
    focused: bool,
    /// The value cell the edit was started from
    return_focus: Option<egui::Id>,
}

#[cfg(feature = "gui")]
fn theme_from_name(name: &str) -> Option<egui::ThemePreference> {
    match name {
        "system" => Some(egui::ThemePreference::System),
        "light" => Some(egui::ThemePreference::Light),
        "dark" => Some(egui::ThemePreference::Dark),
        _ => None,
    }
}

#[cfg(feature = "gui")]
impl App {
    fn new(initial_path: Option<String>, forced_lang: Option<String>) -> Self {
        let saved = settings::Settings::load();
        let lang = forced_lang
            .or_else(|| {
                saved
                    .lang
                    .clone()
                    .filter(|l| i18n::AVAILABLE_LANGUAGES.iter().any(|(c, _)| c == l))
            })
            .unwrap_or_else(i18n::detect_system_language);
        let theme = saved
            .theme
            .as_deref()
            .and_then(theme_from_name)
            .unwrap_or(egui::ThemePreference::System);
        let mut app = Self {
            // tag descriptions and values in the language, by ExifTool
            et: ExifTool::with_options(Self::options(&lang)),
            files: Vec::new(),
            current: 0,
            tags: Vec::new(),
            groups: Vec::new(),
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
            theme,
            applied_theme: None,
            window_title: String::new(),
            refocus: None,
            toolbar_end_width: 0.0,
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

    /// Opens a file picked in the file dialog, with the other files of its
    /// folder to go through.
    fn pick_file(&mut self) {
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

    fn pick_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(&path);
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
        self.writable_tags = if let Some(ft) = exiftool_rs::file_type::detect_from_extension(ext) {
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

    fn go_to(&mut self, index: usize) {
        if index < self.files.len() && index != self.current {
            self.current = index;
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

    /// The settings file, from what the toolbar shows now.
    fn save_settings(&self) {
        settings::Settings {
            theme: Some(self.theme.name().to_string()),
            lang: Some(self.lang.clone()),
        }
        .save();
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

    /// Arabic is written right to left: the table and the bars are mirrored.
    fn is_rtl(&self) -> bool {
        self.lang == "ar"
    }

    fn text(&self, key: &'static str) -> &'static str {
        i18n::ui_text(&self.lang, key)
    }

    /// What a screen reader says for a value cell: the description and the
    /// value (the name), and whether it can be edited (the description).
    fn accessible_value(&self, description: &str, value: &str, writable: bool, edited: bool) -> (String, String) {
        let name = format!("{description}: {value}");
        let state = match (writable, edited) {
            (_, true) => format!("{}, {}", self.text("modified"), self.text("editable")),
            (true, false) => self.text("editable").to_string(),
            (false, false) => self.text("read_only_short").to_string(),
        };
        (name, state)
    }

    /// The Save button and Ctrl+S: write the pending edits to the file.
    fn save_edits(&mut self) {
        if self.pending_edits.is_empty() {
            return;
        }
        let Some(path) = self.files.get(self.current) else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        // (in the current language: ExifTool takes the values back in the
        // language it shows them in)
        let mut et = ExifTool::with_options(Self::options(&self.lang));
        for (tag, value) in &self.pending_edits {
            et.set_new_value(tag, Some(value));
        }
        match et.write_info(&path_str, &path_str) {
            Ok(_) => {
                let n = self.pending_edits.len();
                self.pending_edits.clear();
                self.load_current();
                self.status = format!("{} {}", n, self.text("saved"));
                self.flash_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
            }
            Err(e) => {
                self.status = format!("{}: {}", self.text("save_error"), e);
            }
        }
    }

    fn copy_all(&mut self, ctx: &egui::Context) {
        let text: String = self
            .tags
            .iter()
            .map(|t| format!("{}: {}", t.name, t.print_value))
            .collect::<Vec<_>>()
            .join("\n");
        ctx.copy_text(text);
        self.status = self.text("copied").to_string();
        self.flash_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
    }

    /// The fonts: egui's, then the Noto fonts of the CJK, Arabic,
    /// Devanagari and Bengali scripts (downloaded at build time).
    fn font_definitions() -> egui::FontDefinitions {
        let mut fonts = egui::FontDefinitions::default();
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
        fonts
    }

    /// The keyboard shortcuts that are not a widget's own.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let dialog_open = self.editing.is_some() || self.show_about;
        let nothing_focused = ctx.memory(|m| m.focused()).is_none();
        let (mut open, mut save, mut step, mut jump) = (false, false, 0isize, None);
        ctx.input_mut(|i| {
            if dialog_open {
                return;
            }
            open = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O));
            save = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S));
            if i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
                step = -1;
            }
            if i.consume_key(Modifiers::ALT, Key::ArrowRight) {
                step = 1;
            }
            if nothing_focused && i.modifiers.is_none() {
                if i.key_pressed(Key::ArrowLeft) {
                    step = -1;
                }
                if i.key_pressed(Key::ArrowRight) {
                    step = 1;
                }
                if i.key_pressed(Key::Home) {
                    jump = Some(0);
                }
                if i.key_pressed(Key::End) {
                    jump = Some(usize::MAX);
                }
            }
        });
        // (in Arabic the arrows point the other way)
        if self.is_rtl() {
            step = -step;
        }
        if step != 0 {
            self.navigate(step);
        }
        match jump {
            Some(0) => self.go_to(0),
            Some(_) => self.go_to(self.files.len().saturating_sub(1)),
            None => {}
        }
        if save {
            self.save_edits();
        }
        if open {
            self.pick_file();
        }
    }

    /// A row of a bar: as high as a button, laid out from the start side
    /// (the right in Arabic). `with_layout` would take the whole height.
    fn bar_row<R>(ui: &mut egui::Ui, rtl: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let layout = if rtl {
            Layout::right_to_left(Align::Center)
        } else {
            Layout::left_to_right(Align::Center)
        };
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(size, layout, add).inner
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let ctx = ui.ctx().clone();
        let rtl = self.is_rtl();
        let with_shortcut = |text: &str, key: Key| {
            format!("{} ({})", text, ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key)))
        };
        let mut wrap = false;
        ui.add_space(4.0);
        Self::bar_row(ui, rtl, |ui| {
            if ui
                .button(self.text("open"))
                .on_hover_text(with_shortcut(self.text("tooltip_open"), Key::O))
                .clicked()
            {
                self.pick_file();
            }
            if ui
                .button(self.text("folder"))
                .on_hover_text(self.text("tooltip_folder"))
                .clicked()
            {
                self.pick_folder();
            }
            ui.separator();
            if ui
                .add_enabled(!self.tags.is_empty(), egui::Button::new(self.text("copy")))
                .on_hover_text(self.text("tooltip_copy"))
                .clicked()
            {
                self.copy_all(&ctx);
            }
            let pending = self.pending_edits.len();
            let save_label = if pending > 0 {
                format!("{} ({})", self.text("save"), pending)
            } else {
                self.text("save").to_string()
            };
            if ui
                .add_enabled(pending > 0, egui::Button::new(save_label))
                .on_hover_text(with_shortcut(self.text("tooltip_save"), Key::S))
                .on_disabled_hover_text(self.text("tooltip_save"))
                .clicked()
            {
                self.save_edits();
            }

            // the end of the bar, on the same row if it fits, else on a row
            // of its own
            if ui.available_width() >= self.toolbar_end_width {
                self.toolbar_end(ui);
            } else {
                wrap = true;
            }
        });
        if wrap {
            Self::bar_row(ui, rtl, |ui| self.toolbar_end(ui));
        }
        ui.add_space(2.0);
    }

    /// The language and theme selectors and About, at the end of the
    /// toolbar, added in reading order (and so in Tab order) after a space
    /// as wide as what they leave.
    fn toolbar_end(&mut self, ui: &mut egui::Ui) {
        let spare = ui.available_width() - self.toolbar_end_width;
        if spare > 0.0 {
            ui.add_space(spare);
        }
        let group = ui.scope(|ui| {
        // Language selector (each language by its own name, and in
        // its own language for a screen reader)
        let current_name = self
            .languages
            .iter()
            .find(|(c, _)| *c == self.lang)
            .map(|(_, n)| *n)
            .unwrap_or("English");
        let mut new_lang: Option<&str> = None;
        let combo = egui::ComboBox::from_id_salt("language")
            .selected_text(format!("🌐 {current_name}"))
            .height(f32::INFINITY)
            .show_ui(ui, |ui| {
                for (code, name) in &self.languages {
                    let r = ui.selectable_label(self.lang == *code, *name);
                    ui.ctx().accesskit_node_builder(r.id, |node| {
                        node.set_language(i18n::bcp47(code));
                    });
                    if r.clicked() {
                        new_lang = Some(code);
                    }
                }
            })
            .response
            .on_hover_text(self.text("language"));
        // (the value without the globe, which a screen reader would read)
        ui.ctx().accesskit_node_builder(combo.id, |node| node.set_value(current_name));
        if let Some(l) = new_lang {
            if l != self.lang {
                self.set_language(l);
                self.save_settings();
            }
        }

        // Theme selector
        let mut theme = self.theme;
        let theme_name = |t: egui::ThemePreference| match t {
            egui::ThemePreference::System => self.text("theme_system"),
            egui::ThemePreference::Light => self.text("theme_light"),
            egui::ThemePreference::Dark => self.text("theme_dark"),
        };
        // (egui's own icons end "☀" with a variation selector)
        let theme_icon = |t: egui::ThemePreference| match t {
            egui::ThemePreference::System => "💻",
            egui::ThemePreference::Light => "☀",
            egui::ThemePreference::Dark => "🌙",
        };
        let combo = egui::ComboBox::from_id_salt("theme")
            .selected_text(format!("{} {}", theme_icon(theme), theme_name(theme)))
            .show_ui(ui, |ui| {
                for t in [
                    egui::ThemePreference::System,
                    egui::ThemePreference::Light,
                    egui::ThemePreference::Dark,
                ] {
                    let selected = theme == t;
                    let r = ui.selectable_value(&mut theme, t, format!("{} {}", theme_icon(t), theme_name(t)));
                    r.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, theme_name(t)));
                }
            })
            .response
            .on_hover_text(self.text("theme"));
        ui.ctx().accesskit_node_builder(combo.id, |node| node.set_value(theme_name(self.theme)));
        if theme != self.theme {
            self.theme = theme;
            self.save_settings();
        }

        if ui.button(self.text("about")).clicked() {
            self.show_about = true;
        }
        });
        let width = group.response.rect.width() + ui.spacing().item_spacing.x;
        if (width - self.toolbar_end_width).abs() > 0.5 {
            self.toolbar_end_width = width;
            ui.ctx().request_repaint();
        }
    }

    fn navigation_bar(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let rtl = self.is_rtl();
        ui.add_space(4.0);
        Self::bar_row(ui, rtl, |ui| {
            let several = self.files.len() > 1;
            // the arrows point the way the files go: right to left in Arabic
            let (prev_icon, next_icon) = if rtl { ("▶", "◀") } else { ("◀", "▶") };
            let arrow = |ui: &mut egui::Ui, icon: &str, enabled: bool, name: &'static str| {
                let r = ui.add_enabled(enabled, egui::Button::new(icon).min_size(egui::vec2(28.0, 0.0)));
                // the icon is no name: say what the button does
                r.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
                r.on_hover_text(name).on_disabled_hover_text(name).clicked()
            };
            if several && arrow(ui, prev_icon, self.current > 0, self.text("prev_file")) {
                self.navigate(-1);
            }
            let filename = self
                .files
                .get(self.current)
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            ui.label(RichText::new(filename).strong());
            if several {
                ui.label(RichText::new(format!("{} / {}", self.current + 1, self.files.len())).color(pal.muted));
            }
            if several && arrow(ui, next_icon, self.current + 1 < self.files.len(), self.text("next_file")) {
                self.navigate(1);
            }
        });
        // the legend of the marks (a word and a mark, not a color)
        Self::bar_row(ui, rtl, |ui| {
            let mut legend = egui::text::LayoutJob::default();
            let style = ui.style().clone();
            RichText::new(format!("{EDITABLE_MARK}  ")).color(pal.writable).append_to(
                &mut legend,
                &style,
                egui::FontSelection::Default,
                Align::Center,
            );
            RichText::new(self.text("editable_legend")).color(pal.muted).append_to(
                &mut legend,
                &style,
                egui::FontSelection::Default,
                Align::Center,
            );
            ui.label(legend);
        });
        ui.add_space(2.0);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let flashing = self.flash_until.is_some_and(|t| t > std::time::Instant::now());
        if flashing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        Self::bar_row(ui, self.is_rtl(), |ui| {
            if flashing {
                ui.label(RichText::new(format!("✔ {}", self.status)).color(pal.success).strong());
            } else {
                ui.label(&self.status);
            }
            if !self.pending_edits.is_empty() {
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} {} {}",
                        MODIFIED_MARK,
                        self.pending_edits.len(),
                        self.text("pending")
                    ))
                    .color(pal.modified),
                );
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.25).max(16.0));
            self.logo(ui, 96.0);
            ui.add_space(12.0);
            ui.label(RichText::new(self.text("welcome_title")).size(24.0).strong());
            ui.add_space(8.0);
            for line in self.text("welcome").split("\\n") {
                ui.label(RichText::new(line).color(pal.muted).size(15.0));
            }
        });
    }

    /// The application's icon. It is drawn in black: on a dark theme it is
    /// shown on a light tile, or it would not be seen.
    fn logo(&self, ui: &mut egui::Ui, size: f32) {
        let Some(tex) = &self.icon_texture else {
            return;
        };
        let image = egui::Image::new((tex.id(), egui::vec2(size, size))).alt_text("exiftool-rs");
        let pad = size * 0.12;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size + 2.0 * pad, size + 2.0 * pad), egui::Sense::hover());
        if ui.visuals().dark_mode {
            ui.painter().rect_filled(rect, size * 0.18, egui::Color32::from_gray(230));
        }
        ui.put(rect.shrink(pad), image);
    }

    /// The tag table: one collapsible section per group, a row per tag.
    fn tag_table(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let groups = std::mem::take(&mut self.groups);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (group_name, group_tags) in &groups {
                let header = egui::CollapsingHeader::new(
                    RichText::new(format!("{}  ({})", group_name, group_tags.len()))
                        .strong()
                        .size(15.0),
                )
                .id_salt(("group", group_name))
                .default_open(true);
                header.show(ui, |ui| {
                    let label_w = (ui.available_width() * 0.4).clamp(110.0, 320.0);
                    for (i, tag) in group_tags.iter().enumerate() {
                        ui.push_id((group_name, i), |ui| self.tag_row(ui, tag, i % 2 == 1, label_w, pal));
                    }
                });
                ui.add_space(6.0);
            }
        });
        self.groups = groups;
    }

    fn tag_row(&mut self, ui: &mut egui::Ui, tag: &Tag, striped: bool, label_w: f32, pal: &Palette) {
        let rtl = self.is_rtl();
        let (row_layout, align) = if rtl {
            (Layout::right_to_left(Align::TOP), Align::Max)
        } else {
            (Layout::left_to_right(Align::TOP), Align::Min)
        };
        let desc = self.translate(&tag.name, &tag.description).to_string();
        let edited = self.pending_edits.iter().find(|(n, _)| n == &tag.name).map(|(_, v)| v.clone());
        let display_value = edited.clone().unwrap_or_else(|| {
            self.translate_value(&tag.group.family0, &tag.group.family1, &tag.name, &tag.print_value)
        });
        let writable = self.is_tag_writable(&tag.name);
        let mark_w = 18.0;

        // painted under the row once its height is known
        let stripe_slot = ui.painter().add(egui::Shape::Noop);
        let highlight_slot = ui.painter().add(egui::Shape::Noop);

        let row = ui.with_layout(row_layout, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.allocate_ui_with_layout(egui::vec2(label_w, 0.0), Layout::top_down(align), |ui| {
                ui.set_width(label_w);
                ui.add(egui::Label::new(RichText::new(&desc).color(pal.label)).wrap());
            });
            // the mark: painted, not a widget (the value's own node says
            // the same to a screen reader)
            let (mark_rect, _) = ui.allocate_exact_size(egui::vec2(mark_w, ui.spacing().interact_size.y * 0.8), egui::Sense::hover());
            let mark = match (&edited, writable) {
                (Some(_), _) => Some((MODIFIED_MARK, pal.modified, 9.0)),
                (None, true) => Some((EDITABLE_MARK, pal.writable, 13.0)),
                (None, false) => None,
            };
            if let Some((m, color, size)) = mark {
                ui.painter().text(
                    mark_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    m,
                    egui::FontId::proportional(size),
                    color,
                );
            }
            let value_w = ui.available_width().max(40.0);
            ui.allocate_ui_with_layout(egui::vec2(value_w, 0.0), Layout::top_down(align), |ui| {
                ui.set_width(value_w);
                let mut text = RichText::new(display_value.as_str());
                text = match (&edited, writable) {
                    (Some(_), _) => text.color(pal.modified).italics(),
                    (None, true) => text.color(pal.writable),
                    (None, false) => text.color(pal.value),
                };
                // (an editable value is not selectable: a double-click
                // edits it rather than selecting a word)
                ui.add(egui::Label::new(text).wrap().selectable(!writable).sense(egui::Sense::click()))
            })
            .inner
        });
        let response = row.inner;

        // accessible name and state, before the tooltip (which would be
        // taken as the description otherwise)
        let (name, state) = self.accessible_value(&desc, &display_value, writable, edited.is_some());
        let role = if writable { Role::Button } else { Role::Label };
        response.widget_info(|| WidgetInfo::labeled(role, true, &name));
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_description(state.clone());
            if writable {
                node.clear_value();
            } else {
                node.set_read_only();
            }
        });

        let visuals = ui.visuals().clone();
        let full = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), row.response.rect.y_range()).expand2(egui::vec2(0.0, 1.0));
        if striped {
            ui.painter().set(stripe_slot, egui::Shape::rect_filled(full, 0.0, theme::stripe(&visuals)));
        }
        let cell = response.rect.expand2(egui::vec2(4.0, 1.0));
        if writable && response.hovered() {
            ui.painter().set(highlight_slot, egui::Shape::rect_filled(cell, 3.0, visuals.widgets.hovered.weak_bg_fill));
        }
        // (the focus as egui keeps it: `has_focus` is also false while the
        // window itself is not focused)
        if ui.memory(|m| m.has_focus(response.id)) {
            // the keyboard focus, as a ring (egui draws none on a label)
            ui.painter().rect_stroke(cell, 3.0, egui::Stroke::new(2.0, visuals.selection.stroke.color), egui::StrokeKind::Outside);
        }
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
        if self.refocus == Some(response.id) {
            response.request_focus();
            self.refocus = None;
        }

        // a double-click, or Enter / Space / a screen reader's "click"
        let activated = response.double_clicked()
            || (response.clicked() && !response.clicked_by(egui::PointerButton::Primary));
        let response = if writable {
            response.on_hover_text(self.text("editable_legend"))
        } else {
            response.on_hover_text(self.text("read_only"))
        };
        if writable && activated {
            self.editing = Some(EditState {
                tag_name: tag.name.clone(),
                description: desc,
                original_value: tag.print_value.clone(),
                new_value: display_value,
                focused: false,
                return_focus: Some(response.id),
            });
        }
    }

    fn edit_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        let lang = self.lang.clone();
        let t = |key: &'static str| i18n::ui_text(&lang, key);
        let (mut commit, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::unique("edit_value")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(240.0, 460.0));
            ui.label(RichText::new(format!("{} — {}", t("edit"), edit.description)).heading());
            ui.label(RichText::new(&edit.tag_name).color(pal.muted).monospace());
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}:", t("original"))).color(pal.label));
                ui.label(RichText::new(&edit.original_value).color(pal.value));
            });
            ui.add_space(6.0);
            let field_label = ui.label(RichText::new(t("new_value")).color(pal.label));
            // (a border of its own: egui's light theme draws none, and the
            // white field would not stand out of the dialog)
            let frame = egui::Frame::new()
                .fill(ui.visuals().text_edit_bg_color())
                .stroke(egui::Stroke::new(1.0, pal.border))
                .corner_radius(3.0)
                .inner_margin(egui::Margin::symmetric(6, 4));
            let mut output = egui::TextEdit::singleline(&mut edit.new_value)
                .desired_width(f32::INFINITY)
                .frame(frame)
                .show(ui);
            if !edit.focused {
                // the value selected, so that typing replaces it
                let all = egui::text::CCursorRange::select_all_str(&edit.new_value);
                output.state.cursor.set_char_range(Some(all));
                output.state.store(ui.ctx(), output.response.response.id);
                output.response.response.request_focus();
                edit.focused = true;
            }
            let field = output.response.response.labelled_by(field_label.id);
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                commit = true;
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(t("ok")).clicked() {
                    commit = true;
                }
                if ui.button(t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if commit {
            self.pending_edits.retain(|(n, _)| n != &edit.tag_name);
            if edit.new_value != edit.original_value {
                self.pending_edits.push((edit.tag_name.clone(), edit.new_value.clone()));
            }
        }
        if commit || cancel {
            self.refocus = edit.return_focus;
            self.editing = None;
        }
    }

    fn about_dialog(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.show_about {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.vertical_centered(|ui| {
                self.logo(ui, 64.0);
                ui.add_space(8.0);
                ui.label(RichText::new("exiftool-rs").size(20.0).strong());
                ui.label(format!("v{}", exiftool_rs::VERSION));
                ui.add_space(8.0);
                ui.label("A pure Rust reimplementation of ExifTool");
                ui.add_space(8.0);
                ui.label(
                    "This program is free software: you can redistribute it and/or modify it \
                     under the terms of the GNU General Public License as published by the Free \
                     Software Foundation, either version 3 of the License, or (at your option) \
                     any later version.",
                );
                ui.add_space(4.0);
                ui.label("This program is distributed WITHOUT ANY WARRANTY.");
                ui.add_space(4.0);
                ui.hyperlink_to("Full license text (GPLv3)", "https://www.gnu.org/licenses/gpl-3.0.html");
                ui.add_space(8.0);
                ui.separator();
                ui.label("Based on ExifTool by Phil Harvey");
                ui.hyperlink_to("exiftool.org", "https://exiftool.org/");
                ui.add_space(4.0);
                ui.hyperlink_to("GitHub", "https://github.com/Le-Syl21/exiftool-rs");
                ui.add_space(4.0);
                ui.hyperlink_to("Discord — community & support", "https://discord.gg/T37DYHmt2j");
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Sylvain (project creator) + Claude (implementation)")
                        .small()
                        .color(pal.muted),
                );
                ui.add_space(8.0);
                let ok = ui.button(self.text("ok"));
                if ok.clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.show_about = false;
        }
    }

    /// The whole window, one frame (`eframe::App::ui` with no `Frame`, so
    /// that the tests can run it).
    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Configure fonts for CJK, Arabic, Devanagari and Bengali, and the
        // visuals of both themes
        if !self.fonts_configured {
            self.fonts_configured = true;
            ctx.set_fonts(Self::font_definitions());
            theme::install(&ctx);
        }
        if self.applied_theme != Some(self.theme) {
            ctx.set_theme(self.theme);
            self.applied_theme = Some(self.theme);
        }

        // The window title names the file (screen readers say it too)
        let title = match self.files.get(self.current).and_then(|p| p.file_name()) {
            Some(name) => format!("{} — exiftool-rs", name.to_string_lossy()),
            None => "exiftool-rs".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        // Handle dropped files
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|d| d.path().to_path_buf()));
        if let Some(path) = dropped {
            if path.is_dir() {
                self.open_folder(&path);
            } else if let Some(parent) = path.parent() {
                // Open the file's parent folder and navigate to it
                self.open_folder(parent);
                if let Some(idx) = self.files.iter().position(|f| f == &path) {
                    self.current = idx;
                    self.load_current();
                }
            } else {
                self.open_file(&path);
            }
        }

        self.shortcuts(&ctx);

        // Load icon texture once
        if self.icon_texture.is_none() {
            let icon_data = include_bytes!("../../../assets/icon.png");
            if let Ok(img) = image::load_from_memory(icon_data) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba.into_raw());
                self.icon_texture = Some(ctx.load_texture("icon", color_image, egui::TextureOptions::LINEAR));
            }
        }

        let pal = Palette::of(ui.visuals());

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        if !self.files.is_empty() {
            egui::Panel::top("navigation").show(ui, |ui| self.navigation_bar(ui, &pal));
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(2.0);
            self.status_bar(ui, &pal);
            ui.add_space(2.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            if self.files.is_empty() {
                self.welcome(ui, &pal);
            } else {
                self.tag_table(ui, &pal);
            }
        });

        self.edit_dialog(&ctx, &pal);
        self.about_dialog(&ctx, &pal);
    }
}

#[cfg(feature = "gui")]
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

// The model with no window: what the panels would show, as App holds it.
#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    const CANON: &str = "tests/images/Canon.jpg";

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

    // --- the window, headless: what a screen reader is given, and the
    // keyboard ---

    use egui::accesskit;

    /// A node of the accessibility tree, as a screen reader would read it.
    #[derive(Debug, Clone)]
    struct Node {
        id: accesskit::NodeId,
        role: accesskit::Role,
        label: String,
        value: String,
        description: String,
        labelled_by: Vec<accesskit::NodeId>,
        read_only: bool,
    }

    impl Node {
        /// What names the node: its label, or (a label) its value, or the
        /// node it is labelled by.
        fn name(&self, tree: &[Node]) -> String {
            if !self.label.is_empty() {
                return self.label.clone();
            }
            if let Some(by) = self.labelled_by.first() {
                if let Some(n) = tree.iter().find(|n| n.id == *by) {
                    return if n.label.is_empty() { n.value.clone() } else { n.label.clone() };
                }
            }
            if self.role == accesskit::Role::Label {
                return self.value.clone();
            }
            String::new()
        }
    }

    /// Runs the window for a few frames (fonts and layout settle on the
    /// second) and returns egui's last accessibility tree.
    fn run(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<Node> {
        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(820.0, 760.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input(events), |ui| app.show(ui));
        for _ in 0..2 {
            out = ctx.run_ui(input(Vec::new()), |ui| app.show(ui));
        }
        let update = out.platform_output.accesskit_update.expect("an accessibility tree");
        update
            .nodes
            .into_iter()
            .map(|(id, n)| Node {
                id,
                role: n.role(),
                label: n.label().unwrap_or_default().to_string(),
                value: n.value().unwrap_or_default().to_string(),
                description: n.description().unwrap_or_default().to_string(),
                labelled_by: n.labelled_by().to_vec(),
                read_only: n.is_read_only(),
            })
            .collect()
    }

    fn headless() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn click(node: &Node) -> egui::Event {
        egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node.id,
            data: None,
        })
    }

    /// Two copies of Canon.jpg in a folder, to have files to go through.
    fn two_files(name: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/scratch").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a.jpg", "b.jpg"] {
            std::fs::copy(CANON, dir.join(f)).unwrap();
        }
        dir
    }

    #[test]
    fn every_control_has_an_accessible_name() {
        let dir = two_files("gui_a11y_names");
        let mut app = App::new(Some(dir.to_string_lossy().into_owned()), Some("fr".to_string()));
        let ctx = headless();
        let tree = run(&mut app, &ctx, Vec::new());
        let interactive = [
            accesskit::Role::Button,
            accesskit::Role::ComboBox,
            accesskit::Role::DisclosureTriangle,
            accesskit::Role::TextInput,
            accesskit::Role::CheckBox,
            accesskit::Role::Link,
        ];
        let unnamed: Vec<&Node> = tree
            .iter()
            .filter(|n| interactive.contains(&n.role) && n.name(&tree).trim().is_empty())
            .collect();
        assert!(unnamed.is_empty(), "{unnamed:#?}");
        // and no control is named by an icon only
        for n in tree.iter().filter(|n| interactive.contains(&n.role)) {
            let name = n.name(&tree);
            assert!(name.chars().any(char::is_alphanumeric), "{n:?}");
        }
        let names: Vec<(accesskit::Role, String)> = tree.iter().map(|n| (n.role, n.name(&tree))).collect();
        for (role, name) in [
            (accesskit::Role::Button, "Ouvrir"),
            (accesskit::Role::Button, "Dossier"),
            (accesskit::Role::Button, "Copier"),
            (accesskit::Role::Button, "Enregistrer"),
            (accesskit::Role::Button, "Fichier précédent"),
            (accesskit::Role::Button, "Fichier suivant"),
            (accesskit::Role::ComboBox, "Langue"),
            (accesskit::Role::ComboBox, "Thème"),
            (accesskit::Role::DisclosureTriangle, "EXIF"),
        ] {
            assert!(
                names.iter().any(|(r, n)| *r == role && n.starts_with(name)),
                "{role:?} {name}: {names:#?}"
            );
        }
    }

    #[test]
    fn a_value_is_read_with_its_description_and_its_state() {
        let mut app = App::new(Some(CANON.to_string()), Some("fr".to_string()));
        let ctx = headless();
        let tree = run(&mut app, &ctx, Vec::new());
        // an editable value: a button, "description: value", "modifiable"
        let make = tree.iter().find(|n| n.label == "Fabricant: Canon").expect("Make");
        assert_eq!(make.role, accesskit::Role::Button);
        assert_eq!(make.description, "modifiable");
        assert!(!make.read_only);
        // a read-only one: text, said to be read-only
        let (desc, value) = {
            let (_, tags) = app.groups.iter().find(|(g, _)| g == "Composite").unwrap();
            let t = tags.iter().find(|t| t.name == "Megapixels").unwrap();
            (t.description.clone(), t.print_value.clone())
        };
        let mp = tree
            .iter()
            .find(|n| n.value == format!("{desc}: {value}"))
            .unwrap_or_else(|| panic!("{desc}: {value}"));
        assert_eq!(mp.role, accesskit::Role::Label);
        assert!(mp.read_only);
        assert_eq!(mp.description, "lecture seule");
        // the group sections say whether they are open
        assert!(tree.iter().any(|n| n.role == accesskit::Role::DisclosureTriangle && n.label.starts_with("Composite")));
    }

    #[test]
    fn a_value_is_edited_with_the_keyboard_alone() {
        let dir = two_files("gui_a11y_keyboard");
        let file = dir.join("a.jpg");
        let mut app = App::new(Some(file.to_string_lossy().into_owned()), Some("en".to_string()));
        let ctx = headless();
        let tree = run(&mut app, &ctx, Vec::new());
        let make = tree.iter().find(|n| n.label == "Make: Canon").expect("Make").clone();
        // a screen reader's "activate" (as Enter or Space on the focused
        // value): the edit dialog opens, its field focused and labelled
        let tree = run(&mut app, &ctx, vec![click(&make)]);
        let edit = app.editing.as_ref().expect("the edit dialog");
        assert_eq!(edit.tag_name, "Make");
        let field = tree.iter().find(|n| n.role == accesskit::Role::TextInput).expect("a text field");
        assert_eq!(field.name(&tree), "New value");
        // Escape cancels
        run(&mut app, &ctx, vec![key(egui::Key::Escape)]);
        assert!(app.editing.is_none() && app.pending_edits.is_empty());
        // again, a new value typed, and Enter confirms
        run(&mut app, &ctx, vec![click(&make)]);
        // (the value is selected when the dialog opens: what is typed replaces it)
        let tree = run(&mut app, &ctx, vec![egui::Event::Text("Kanon".into()), key(egui::Key::Enter)]);
        assert!(app.editing.is_none());
        assert_eq!(app.pending_edits, vec![("Make".to_string(), "Kanon".to_string())]);
        // the value is now said to be modified, and the Save button counts it
        let make = tree.iter().find(|n| n.label == "Make: Kanon").expect("edited Make");
        assert_eq!(make.description, "modified, editable");
        assert!(tree.iter().any(|n| n.role == accesskit::Role::Button && n.label == "Save (1)"));
    }

    #[test]
    fn the_files_are_gone_through_with_the_keyboard() {
        let dir = two_files("gui_a11y_files");
        let mut app = App::new(Some(dir.to_string_lossy().into_owned()), Some("en".to_string()));
        let ctx = headless();
        run(&mut app, &ctx, Vec::new());
        assert_eq!(app.current, 0);
        let alt_right = egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        };
        run(&mut app, &ctx, vec![alt_right]);
        assert_eq!(app.current, 1);
        run(&mut app, &ctx, vec![key(egui::Key::Home)]);
        assert_eq!(app.current, 0);
        run(&mut app, &ctx, vec![key(egui::Key::End)]);
        assert_eq!(app.current, 1);
        assert_eq!(app.window_title, "b.jpg — exiftool-rs");
    }

    #[test]
    fn the_fonts_have_every_glyph_the_window_shows() {
        let mut app = App::new(None, Some("en".to_string()));
        let ctx = headless();
        run(&mut app, &ctx, Vec::new());
        let font = egui::FontId::proportional(14.0);
        let mut missing = Vec::new();
        let mut texts: Vec<String> = [EDITABLE_MARK, MODIFIED_MARK, "◀", "▶", "✔", "—", "🌐", "💻", "☀", "🌙"].iter().map(|s| s.to_string()).collect();
        for (code, name) in i18n::AVAILABLE_LANGUAGES {
            texts.push(name.to_string());
            for key in ["open", "folder", "copy", "save", "about", "editable_legend", "theme", "theme_system", "theme_light", "theme_dark", "language", "prev_file", "next_file", "new_value", "welcome", "pending"] {
                texts.push(i18n::ui_text(code, key).to_string());
            }
        }
        for t in texts {
            if !ctx.fonts_mut(|f| f.has_glyphs(&font, &t)) {
                missing.push(t);
            }
        }
        assert!(missing.is_empty(), "{missing:#?}");
    }
}
