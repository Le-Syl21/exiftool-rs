//! Internationalization support for tag descriptions.
//!
//! Uses YAML locale files in `locales/` directory.
//! Add a new language by creating `locales/xx.yml` with `TagName: "Translation"` entries,
//! then add it to AVAILABLE_LANGUAGES and LOCALES below.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Available languages — add new ones here and they appear in -h automatically
pub const AVAILABLE_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("en_ca", "English (CA)"),
    ("en_gb", "English (UK)"),
    ("fr", "Français"),
    ("es", "Español"),
    ("pt", "Português"),
    ("it", "Italiano"),
    ("de", "Deutsch"),
    ("nl", "Nederlands"),
    ("sv", "Svenska"),
    ("fi", "Suomi"),
    ("pl", "Polski"),
    ("cs", "Čeština"),
    ("sk", "Slovenčina"),
    ("tr", "Türkçe"),
    ("ru", "Русский"),
    ("ar", "العربية"),
    ("hi", "हिन्दी"),
    ("bn", "বাংলা"),
    ("zh", "中文"),
    ("zh_tw", "繁體中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

// Embed locale files at compile time
static LOCALES: &[(&str, &str)] = &[
    ("en_ca", include_str!("../locales/en_ca.yml")),
    ("en_gb", include_str!("../locales/en_gb.yml")),
    ("fr", include_str!("../locales/fr.yml")),
    ("es", include_str!("../locales/es.yml")),
    ("pt", include_str!("../locales/pt.yml")),
    ("it", include_str!("../locales/it.yml")),
    ("de", include_str!("../locales/de.yml")),
    ("nl", include_str!("../locales/nl.yml")),
    ("sv", include_str!("../locales/sv.yml")),
    ("fi", include_str!("../locales/fi.yml")),
    ("pl", include_str!("../locales/pl.yml")),
    ("cs", include_str!("../locales/cs.yml")),
    ("sk", include_str!("../locales/sk.yml")),
    ("tr", include_str!("../locales/tr.yml")),
    ("ru", include_str!("../locales/ru.yml")),
    ("ar", include_str!("../locales/ar.yml")),
    ("hi", include_str!("../locales/hi.yml")),
    ("bn", include_str!("../locales/bn.yml")),
    ("zh", include_str!("../locales/zh.yml")),
    ("zh_tw", include_str!("../locales/zh_tw.yml")),
    ("ja", include_str!("../locales/ja.yml")),
    ("ko", include_str!("../locales/ko.yml")),
];

static PARSED_LOCALES: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn parse_yaml_simple(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key, val)) = line.split_once(": ") {
            let key = key.trim().trim_matches('"');
            let val = val.trim().trim_matches('"');
            if !key.is_empty() && !val.is_empty() {
                map.insert(key.to_string(), val.to_string());
            }
        }
    }
    map
}

fn get_all_locales() -> &'static HashMap<String, HashMap<String, String>> {
    PARSED_LOCALES.get_or_init(|| {
        let mut all = HashMap::new();
        for (code, content) in LOCALES {
            all.insert(code.to_string(), parse_yaml_simple(content));
        }
        all
    })
}

/// Get translations for a language code. Returns None for "en" or unknown languages.
pub fn get_translations(lang: &str) -> Option<HashMap<&'static str, &'static str>> {
    // Normalize lang code
    let lang = match lang {
        "zh_cn" | "zh_CN" | "zhcn" | "zh-cn" | "zh-CN" => "zh",
        "zh_tw" | "zh_TW" | "zhtw" | "zh-tw" | "zh-TW" => "zh_tw",
        "pt_br" | "pt_BR" | "ptbr" | "pt-br" | "pt-BR" => "pt",
        "en_ca" | "en_CA" | "en-ca" | "en-CA" => "en_ca",
        "en_gb" | "en_GB" | "en-gb" | "en-GB" => "en_gb",
        other => other,
    };

    if lang == "en" { return None; }

    let locales = get_all_locales();
    let locale = locales.get(lang)?;

    let leaked: &'static HashMap<String, String> = Box::leak(Box::new(locale.clone()));
    let mut result = HashMap::new();
    for (k, v) in leaked {
        result.insert(k.as_str(), v.as_str());
    }
    Some(result)
}

/// Translate a tag description. Returns the original if no translation exists.
pub fn translate(lang: &str, tag_name: &str, default: &str) -> String {
    if lang == "en" { return default.to_string(); }
    let locales = get_all_locales();
    if let Some(locale) = locales.get(lang) {
        if let Some(translation) = locale.get(tag_name) {
            return translation.clone();
        }
    }
    default.to_string()
}

/// Detect system language for GUI autodetection.
/// Returns the language code (e.g., "fr", "de", "ja") or "en" as fallback.
pub fn detect_system_language() -> String {
    // Check LANG, LC_ALL, LC_MESSAGES environment variables
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let val = val.to_lowercase();
            // Parse "fr_FR.UTF-8" → "fr"
            let code = val.split('.').next().unwrap_or(&val);
            let code = code.split('_').next().unwrap_or(code);
            // Check if we support this language
            if AVAILABLE_LANGUAGES.iter().any(|(c, _)| *c == code) {
                return code.to_string();
            }
            // Try with full code (zh_tw, en_ca, etc.)
            let full = val.split('.').next().unwrap_or(&val).replace('-', "_");
            if AVAILABLE_LANGUAGES.iter().any(|(c, _)| *c == full) {
                return full;
            }
        }
    }
    "en".to_string()
}

/// List available language codes
pub fn available_languages() -> Vec<(&'static str, &'static str)> {
    AVAILABLE_LANGUAGES.to_vec()
}

/// GUI interface translations (buttons, messages, labels)
pub fn ui_text<'a>(lang: &str, key: &'a str) -> &'a str {
    match (lang, key) {
        // French
        ("fr", "open") => "📂 Ouvrir",
        ("fr", "folder") => "📁 Dossier",
        ("fr", "copy") => "📋 Copier",
        ("fr", "save") => "💾 Enregistrer",
        ("fr", "welcome") => "Glissez un fichier ou dossier ici\nou utilisez Ouvrir / Dossier",
        ("fr", "welcome_title") => "exiftool-rs",
        ("fr", "no_files") => "Aucun fichier supporté dans le dossier",
        ("fr", "read_only") => "Lecture seule (tag calculé/composite)",
        ("fr", "original") => "Original",
        ("fr", "ok") => "OK",
        ("fr", "cancel") => "Annuler",
        ("fr", "edit") => "Modifier",
        ("fr", "pending") => "modification(s) en attente",
        ("fr", "saved") => "modifications enregistrées",
        ("fr", "save_error") => "Erreur d'enregistrement",
        ("fr", "error") => "Erreur",
        ("fr", "drop_start") => "Glissez un fichier ou dossier pour commencer",
        // Spanish
        ("es", "open") => "📂 Abrir",
        ("es", "folder") => "📁 Carpeta",
        ("es", "copy") => "📋 Copiar",
        ("es", "save") => "💾 Guardar",
        ("es", "welcome") => "Arrastre un archivo o carpeta aquí\no use Abrir / Carpeta",
        ("es", "no_files") => "No se encontraron archivos compatibles",
        ("es", "read_only") => "Solo lectura (tag calculado)",
        ("es", "original") => "Original",
        ("es", "ok") => "Aceptar",
        ("es", "cancel") => "Cancelar",
        ("es", "edit") => "Editar",
        ("es", "pending") => "modificación(es) pendiente(s)",
        ("es", "saved") => "modificaciones guardadas",
        ("es", "save_error") => "Error al guardar",
        ("es", "error") => "Error",
        ("es", "drop_start") => "Arrastre un archivo o carpeta para empezar",
        // Portuguese
        ("pt", "open") => "📂 Abrir",
        ("pt", "folder") => "📁 Pasta",
        ("pt", "copy") => "📋 Copiar",
        ("pt", "save") => "💾 Salvar",
        ("pt", "welcome") => "Arraste um arquivo ou pasta aqui\nou use Abrir / Pasta",
        ("pt", "no_files") => "Nenhum arquivo compatível encontrado",
        ("pt", "read_only") => "Somente leitura (tag calculado)",
        ("pt", "ok") => "OK",
        ("pt", "cancel") => "Cancelar",
        ("pt", "edit") => "Editar",
        ("pt", "pending") => "modificação(ões) pendente(s)",
        ("pt", "error") => "Erro",
        ("pt", "drop_start") => "Arraste um arquivo ou pasta para começar",
        // Italian
        ("it", "open") => "📂 Apri",
        ("it", "folder") => "📁 Cartella",
        ("it", "copy") => "📋 Copia",
        ("it", "save") => "💾 Salva",
        ("it", "welcome") => "Trascina un file o una cartella qui\no usa Apri / Cartella",
        ("it", "no_files") => "Nessun file supportato trovato",
        ("it", "read_only") => "Sola lettura (tag calcolato)",
        ("it", "ok") => "OK",
        ("it", "cancel") => "Annulla",
        ("it", "edit") => "Modifica",
        ("it", "pending") => "modifica/e in sospeso",
        ("it", "error") => "Errore",
        ("it", "drop_start") => "Trascina un file o una cartella per iniziare",
        // German
        ("de", "open") => "📂 Öffnen",
        ("de", "folder") => "📁 Ordner",
        ("de", "copy") => "📋 Kopieren",
        ("de", "save") => "💾 Speichern",
        ("de", "welcome") => "Datei oder Ordner hierher ziehen\noder Öffnen / Ordner verwenden",
        ("de", "no_files") => "Keine unterstützten Dateien gefunden",
        ("de", "read_only") => "Schreibgeschützt (berechneter Tag)",
        ("de", "ok") => "OK",
        ("de", "cancel") => "Abbrechen",
        ("de", "edit") => "Bearbeiten",
        ("de", "pending") => "Änderung(en) ausstehend",
        ("de", "saved") => "Änderungen gespeichert",
        ("de", "save_error") => "Speicherfehler",
        ("de", "error") => "Fehler",
        ("de", "drop_start") => "Datei oder Ordner ziehen zum Starten",
        // Russian
        ("ru", "open") => "📂 Открыть",
        ("ru", "folder") => "📁 Папка",
        ("ru", "copy") => "📋 Копировать",
        ("ru", "save") => "💾 Сохранить",
        ("ru", "welcome") => "Перетащите файл или папку сюда\nили используйте Открыть / Папка",
        ("ru", "no_files") => "Поддерживаемые файлы не найдены",
        ("ru", "read_only") => "Только чтение (вычисляемый тег)",
        ("ru", "ok") => "ОК",
        ("ru", "cancel") => "Отмена",
        ("ru", "edit") => "Редактировать",
        ("ru", "pending") => "изменение(й) ожидает(ют)",
        ("ru", "error") => "Ошибка",
        ("ru", "drop_start") => "Перетащите файл или папку для начала",
        // Japanese
        ("ja", "open") => "📂 開く",
        ("ja", "folder") => "📁 フォルダ",
        ("ja", "copy") => "📋 コピー",
        ("ja", "save") => "💾 保存",
        ("ja", "welcome") => "ファイルまたはフォルダをここにドロップ\nまたは開く/フォルダボタンを使用",
        ("ja", "no_files") => "対応ファイルが見つかりません",
        ("ja", "read_only") => "読み取り専用（計算タグ）",
        ("ja", "ok") => "OK",
        ("ja", "cancel") => "キャンセル",
        ("ja", "edit") => "編集",
        ("ja", "pending") => "件の変更が保留中",
        ("ja", "error") => "エラー",
        ("ja", "drop_start") => "ファイルまたはフォルダをドロップして開始",
        // Korean
        ("ko", "open") => "📂 열기",
        ("ko", "folder") => "📁 폴더",
        ("ko", "copy") => "📋 복사",
        ("ko", "save") => "💾 저장",
        ("ko", "welcome") => "파일이나 폴더를 여기에 드롭하세요\n또는 열기 / 폴더 버튼 사용",
        ("ko", "no_files") => "지원되는 파일을 찾을 수 없습니다",
        ("ko", "read_only") => "읽기 전용 (계산된 태그)",
        ("ko", "ok") => "확인",
        ("ko", "cancel") => "취소",
        ("ko", "edit") => "편집",
        ("ko", "pending") => "건 변경 대기 중",
        ("ko", "error") => "오류",
        ("ko", "drop_start") => "파일이나 폴더를 드롭하여 시작",
        // Chinese Simplified
        ("zh", "open") => "📂 打开",
        ("zh", "folder") => "📁 文件夹",
        ("zh", "copy") => "📋 复制",
        ("zh", "save") => "💾 保存",
        ("zh", "welcome") => "将文件或文件夹拖放到此处\n或使用打开/文件夹按钮",
        ("zh", "no_files") => "未找到支持的文件",
        ("zh", "read_only") => "只读（计算标签）",
        ("zh", "ok") => "确定",
        ("zh", "cancel") => "取消",
        ("zh", "edit") => "编辑",
        ("zh", "pending") => "项修改待保存",
        ("zh", "error") => "错误",
        ("zh", "drop_start") => "拖放文件或文件夹以开始",
        // Arabic
        ("ar", "open") => "📂 فتح",
        ("ar", "folder") => "📁 مجلد",
        ("ar", "copy") => "📋 نسخ",
        ("ar", "save") => "💾 حفظ",
        ("ar", "welcome") => "اسحب ملفًا أو مجلدًا إلى هنا\nأو استخدم فتح / مجلد",
        ("ar", "read_only") => "للقراءة فقط (علامة محسوبة)",
        ("ar", "ok") => "موافق",
        ("ar", "cancel") => "إلغاء",
        ("ar", "edit") => "تعديل",
        ("ar", "pending") => "تعديل(ات) معلقة",
        ("ar", "error") => "خطأ",
        ("ar", "drop_start") => "اسحب ملفًا أو مجلدًا للبدء",
        // Hindi
        ("hi", "open") => "📂 खोलें",
        ("hi", "folder") => "📁 फ़ोल्डर",
        ("hi", "copy") => "📋 कॉपी",
        ("hi", "save") => "💾 सहेजें",
        ("hi", "welcome") => "यहाँ फ़ाइल या फ़ोल्डर खींचें\nया खोलें / फ़ोल्डर बटन उपयोग करें",
        ("hi", "read_only") => "केवल पढ़ने के लिए (गणना टैग)",
        ("hi", "ok") => "ठीक",
        ("hi", "cancel") => "रद्द करें",
        ("hi", "edit") => "संपादित करें",
        ("hi", "pending") => "संशोधन लंबित",
        ("hi", "error") => "त्रुटि",
        ("hi", "drop_start") => "शुरू करने के लिए फ़ाइल या फ़ोल्डर खींचें",
        // Bengali
        ("bn", "open") => "📂 খুলুন",
        ("bn", "folder") => "📁 ফোল্ডার",
        ("bn", "copy") => "📋 কপি",
        ("bn", "save") => "💾 সংরক্ষণ",
        ("bn", "welcome") => "এখানে ফাইল বা ফোল্ডার টেনে আনুন\nঅথবা খুলুন / ফোল্ডার বোতাম ব্যবহার করুন",
        ("bn", "read_only") => "শুধুমাত্র পঠনযোগ্য (গণনাকৃত ট্যাগ)",
        ("bn", "ok") => "ঠিক আছে",
        ("bn", "cancel") => "বাতিল",
        ("bn", "edit") => "সম্পাদনা",
        ("bn", "pending") => "টি পরিবর্তন মুলতুবি",
        ("bn", "error") => "ত্রুটি",
        ("bn", "drop_start") => "শুরু করতে ফাইল বা ফোল্ডার টেনে আনুন",
        // Dutch
        ("nl", "open") => "📂 Openen",
        ("nl", "folder") => "📁 Map",
        ("nl", "copy") => "📋 Kopiëren",
        ("nl", "save") => "💾 Opslaan",
        ("nl", "welcome") => "Sleep een bestand of map hierheen\nof gebruik Openen / Map",
        ("nl", "ok") => "OK",
        ("nl", "cancel") => "Annuleren",
        ("nl", "edit") => "Bewerken",
        ("nl", "error") => "Fout",
        ("nl", "drop_start") => "Sleep een bestand of map om te starten",
        // Swedish
        ("sv", "open") => "📂 Öppna",
        ("sv", "folder") => "📁 Mapp",
        ("sv", "copy") => "📋 Kopiera",
        ("sv", "save") => "💾 Spara",
        ("sv", "welcome") => "Dra en fil eller mapp hit\neller använd Öppna / Mapp",
        ("sv", "ok") => "OK",
        ("sv", "cancel") => "Avbryt",
        ("sv", "edit") => "Redigera",
        ("sv", "error") => "Fel",
        ("sv", "drop_start") => "Dra en fil eller mapp för att börja",
        // Polish
        ("pl", "open") => "📂 Otwórz",
        ("pl", "folder") => "📁 Folder",
        ("pl", "copy") => "📋 Kopiuj",
        ("pl", "save") => "💾 Zapisz",
        ("pl", "welcome") => "Przeciągnij plik lub folder tutaj\nlub użyj Otwórz / Folder",
        ("pl", "ok") => "OK",
        ("pl", "cancel") => "Anuluj",
        ("pl", "edit") => "Edytuj",
        ("pl", "error") => "Błąd",
        ("pl", "drop_start") => "Przeciągnij plik lub folder, aby rozpocząć",
        // Turkish
        ("tr", "open") => "📂 Aç",
        ("tr", "folder") => "📁 Klasör",
        ("tr", "copy") => "📋 Kopyala",
        ("tr", "save") => "💾 Kaydet",
        ("tr", "welcome") => "Bir dosya veya klasör sürükleyin\nveya Aç / Klasör düğmelerini kullanın",
        ("tr", "ok") => "Tamam",
        ("tr", "cancel") => "İptal",
        ("tr", "edit") => "Düzenle",
        ("tr", "error") => "Hata",
        ("tr", "drop_start") => "Başlamak için dosya veya klasör sürükleyin",
        // Czech
        ("cs", "open") => "📂 Otevřít",
        ("cs", "folder") => "📁 Složka",
        ("cs", "copy") => "📋 Kopírovat",
        ("cs", "save") => "💾 Uložit",
        ("cs", "welcome") => "Přetáhněte soubor nebo složku sem\nnebo použijte Otevřít / Složka",
        ("cs", "ok") => "OK",
        ("cs", "cancel") => "Zrušit",
        ("cs", "edit") => "Upravit",
        ("cs", "error") => "Chyba",
        ("cs", "drop_start") => "Přetáhněte soubor nebo složku pro zahájení",
        // Slovak
        ("sk", "open") => "📂 Otvoriť",
        ("sk", "folder") => "📁 Priečinok",
        ("sk", "copy") => "📋 Kopírovať",
        ("sk", "save") => "💾 Uložiť",
        ("sk", "ok") => "OK",
        ("sk", "cancel") => "Zrušiť",
        ("sk", "edit") => "Upraviť",
        ("sk", "error") => "Chyba",
        ("sk", "drop_start") => "Presuňte súbor alebo priečinok pre spustenie",
        // Finnish
        ("fi", "open") => "📂 Avaa",
        ("fi", "folder") => "📁 Kansio",
        ("fi", "copy") => "📋 Kopioi",
        ("fi", "save") => "💾 Tallenna",
        ("fi", "ok") => "OK",
        ("fi", "cancel") => "Peruuta",
        ("fi", "edit") => "Muokkaa",
        ("fi", "error") => "Virhe",
        ("fi", "drop_start") => "Vedä tiedosto tai kansio aloittaaksesi",
        // Default English fallback
        (_, "open") => "📂 Open",
        (_, "folder") => "📁 Folder",
        (_, "copy") => "📋 Copy",
        (_, "save") => "💾 Save",
        (_, "welcome") => "Drop a file or folder here\nor use Open / Folder buttons",
        (_, "welcome_title") => "exiftool-rs",
        (_, "no_files") => "No supported files found in folder",
        (_, "read_only") => "Read-only (composite/computed tag)",
        (_, "original") => "Original",
        (_, "ok") => "OK",
        (_, "cancel") => "Cancel",
        (_, "edit") => "Edit",
        (_, "pending") => "modification(s) pending",
        (_, "saved") => "edits saved",
        (_, "save_error") => "Save error",
        (_, "error") => "Error",
        (_, "drop_start") => "Drop a file or folder to start",
        _ => key,
    }
}
