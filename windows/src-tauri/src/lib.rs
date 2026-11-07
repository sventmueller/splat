use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use splat_file_state::{Encoding, FileFormat};
use splat_markdown::doc::{DiagramKind, Figure};
use splat_markdown::docx::{Picture, TitlePage, WordExport};
use splat_notes::{FileHits, Query, Rewrite, Tree};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tauri::menu::MenuBuilder;
use tauri::{AppHandle, Emitter, Manager};
use std::sync::atomic::AtomicBool;

const MAX_DOCUMENT_BYTES: u64 = 50 * 1024 * 1024;
const MAX_EXPORT_BYTES: usize = 100 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileFormatDto {
    bom: bool,
    crlf: bool,
    encoding: String,
}

impl Default for FileFormatDto {
    fn default() -> Self {
        Self {
            bom: false,
            crlf: false,
            encoding: "utf-8".to_string(),
        }
    }
}

impl FileFormatDto {
    fn from_file_format(format: FileFormat) -> Self {
        let encoding = match format.encoding {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Utf16Be => "utf-16be",
        };
        Self {
            bom: format.bom,
            crlf: format.crlf,
            encoding: encoding.to_string(),
        }
    }

    fn to_file_format(&self) -> Result<FileFormat, String> {
        let encoding = match self.encoding.as_str() {
            "utf-8" => Encoding::Utf8,
            "utf-16le" => Encoding::Utf16Le,
            "utf-16be" => Encoding::Utf16Be,
            other => return Err(format!("Unsupported text encoding: {other}")),
        };
        Ok(FileFormat {
            bom: self.bom,
            crlf: self.crlf,
            encoding,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenedDocument {
    path: String,
    name: String,
    content: String,
    format: FileFormatDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitChange {
    kind: &'static str,
    start_line: usize,
    end_line: usize,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteNodeDto {
    name: String,
    path: String,
    folder: bool,
    children: Vec<NoteNodeDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteTreeDto {
    root: String,
    top: Vec<NoteNodeDto>,
    cut: bool,
    unreadable: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteHitDto {
    line: usize,
    column: usize,
    context: String,
    match_start: usize,
    match_end: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteFileHitsDto {
    path: String,
    hits: Vec<NoteHitDto>,
    more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteSearchDto {
    files: Vec<NoteFileHitsDto>,
    unreadable: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameResultDto {
    old_path: String,
    new_path: String,
    rewritten_files: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FigureAssetDto {
    key: String,
    data: String,
    width: f64,
    height: f64,
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
        .unwrap_or(false)
}

fn decode_document(bytes: Vec<u8>) -> Result<(String, FileFormat), String> {
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The file is larger than the 50 MiB Windows MVP limit.".to_string());
    }
    let (encoding, decoded) = Encoding::read(bytes)
        .ok_or_else(|| "The file is not valid UTF-8 or BOM-marked UTF-16 text.".to_string())?;
    let (mut format, content) = FileFormat::of(&decoded);
    format.encoding = encoding;
    Ok((content.to_string(), format))
}

fn validate_markdown_path(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path);
    if !is_markdown_path(&candidate) {
        return Err("Please choose a Markdown file (.md, .markdown, .mdown or .mkd).".to_string());
    }
    Ok(candidate)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn hex_key(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for byte in text.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn figure_key(figure: &Figure<'_>) -> String {
    let raw = match figure {
        Figure::Math { tex, display } => {
            format!("math:{}:{tex}", if *display { "display" } else { "inline" })
        }
        Figure::Diagram { source, kind } => {
            let kind = match kind {
                DiagramKind::Mermaid => "mermaid",
                DiagramKind::Graphviz => "graphviz",
                DiagramKind::PlantUml => "plantuml",
            };
            format!("diagram:{kind}:{source}")
        }
    };
    hex_key(&raw)
}

fn diagram_name(kind: DiagramKind) -> &'static str {
    match kind {
        DiagramKind::Mermaid => "mermaid",
        DiagramKind::Graphviz => "graphviz",
        DiagramKind::PlantUml => "plantuml",
    }
}

fn is_safe_relative_path(raw: &str) -> bool {
    !raw.trim().is_empty()
        && !raw.contains("://")
        && !raw.starts_with("file:")
        && !Path::new(raw).is_absolute()
        && !raw.split(['/', '\\']).any(|part| part == "..")
}

fn local_path(root: Option<&Path>, raw: &str) -> Option<PathBuf> {
    let root = root?.canonicalize().ok()?;
    let clean = raw.split(['?', '#']).next()?.trim();
    if !is_safe_relative_path(clean) {
        return None;
    }
    let path = root.join(clean).canonicalize().ok()?;
    path.starts_with(&root).then_some(path)
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    let format = image::guess_format(bytes).ok()?;
    match format {
        image::ImageFormat::Png => Some("image/png"),
        image::ImageFormat::Jpeg => Some("image/jpeg"),
        image::ImageFormat::Gif => Some("image/gif"),
        image::ImageFormat::WebP => Some("image/webp"),
        image::ImageFormat::Bmp => Some("image/bmp"),
        _ => None,
    }
}

fn image_data_url(raw: &str, root: Option<&Path>) -> Option<String> {
    let path = local_path(root, raw)?;
    let bytes = fs::read(path).ok()?;
    let mime = image_mime(&bytes)?;
    Some(format!("data:{mime};base64,{}", BASE64.encode(bytes)))
}

fn image_picture(raw: &str, root: Option<&Path>) -> Option<Picture> {
    let path = local_path(root, raw)?;
    let bytes = fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let (width, height) = image.dimensions();
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, image::ImageFormat::Png).ok()?;
    Some(Picture {
        data: encoded.into_inner(),
        jpeg: false,
        // CSS pixels to Word points at 96 DPI.
        width: f64::from(width) * 0.75,
        height: f64::from(height) * 0.75,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoverySnapshot {
    path: Option<String>,
    name: String,
    content: String,
    format: FileFormatDto,
    saved_at: u64,
}

fn recovery_file(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot locate Splat's recovery folder: {error}"))?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Cannot create Splat's recovery folder: {error}"))?;
    Ok(directory.join("recovery.json"))
}

#[tauri::command]
fn save_recovery_snapshot(app: AppHandle, snapshot: RecoverySnapshot) -> Result<(), String> {
    if snapshot.content.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The recovery draft is larger than the 50 MiB limit.".to_string());
    }
    let path = recovery_file(&app)?;
    let bytes = serde_json::to_vec(&snapshot)
        .map_err(|error| format!("Cannot serialize recovery draft: {error}"))?;
    fs::write(path, bytes).map_err(|error| format!("Cannot write recovery draft: {error}"))
}

#[tauri::command]
fn load_recovery_snapshot(app: AppHandle) -> Result<Option<RecoverySnapshot>, String> {
    let path = recovery_file(&app)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot read recovery draft: {error}")),
    };
    let snapshot: RecoverySnapshot = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Recovery draft is damaged: {error}"))?;
    if snapshot.content.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The stored recovery draft exceeds the 50 MiB limit.".to_string());
    }
    Ok(Some(snapshot))
}

#[tauri::command]
fn clear_recovery_snapshot(app: AppHandle) -> Result<(), String> {
    let path = recovery_file(&app)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot clear recovery draft: {error}")),
    }
}

#[tauri::command]
fn startup_file() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| is_markdown_path(path) && path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn tree_node(tree: &Tree, index: usize) -> Option<NoteNodeDto> {
    let node = tree.nodes.get(index)?;
    Some(NoteNodeDto {
        name: node.name.clone(),
        path: node.path.to_string_lossy().into_owned(),
        folder: node.folder,
        children: node.children.iter().filter_map(|child| tree_node(tree, *child)).collect(),
    })
}

fn tree_dto(tree: &Tree) -> NoteTreeDto {
    NoteTreeDto {
        root: tree.root.to_string_lossy().into_owned(),
        top: tree.top.iter().filter_map(|index| tree_node(tree, *index)).collect(),
        cut: tree.cut,
        unreadable: tree.unreadable,
    }
}

fn query_for(mode: &str, query: &str, root: &Path) -> Result<Query, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Enter text or a tag to search for.".to_string());
    }
    match mode {
        "tag" => Ok(Query::Tag(query.trim_start_matches('#').to_string())),
        "links" => {
            let path = PathBuf::from(query);
            let target = if path.is_absolute() { path } else { root.join(path) };
            Ok(Query::LinksTo(target))
        }
        _ => Ok(Query::Text {
            needle: query.to_string(),
            case_sensitive: false,
        }),
    }
}

fn search_dto(files: Vec<FileHits>, unreadable: usize) -> NoteSearchDto {
    NoteSearchDto {
        files: files
            .into_iter()
            .map(|file| NoteFileHitsDto {
                path: file.path.to_string_lossy().into_owned(),
                hits: file
                    .hits
                    .into_iter()
                    .map(|hit| NoteHitDto {
                        line: hit.line + 1,
                        column: hit.column,
                        context: hit.context,
                        match_start: hit.in_context.start,
                        match_end: hit.in_context.end,
                    })
                    .collect(),
                more: file.more,
            })
            .collect(),
        unreadable,
    }
}

fn new_note_path(from: &Path, requested_name: &str) -> Result<PathBuf, String> {
    let name = requested_name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|character| matches!(character, '/' | '\\' | ':' | '\0'))
        || name.starts_with('.')
    {
        return Err("Use a plain file name without a folder path.".to_string());
    }

    let extension = from
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
        .ok_or_else(|| "The source file is not a supported Markdown note.".to_string())?;

    let file_name = if is_markdown_path(Path::new(name)) {
        name.to_string()
    } else {
        format!("{name}.{extension}")
    };
    let target = from.with_file_name(file_name);
    if target == from {
        return Err("The new name is the same as the current name.".to_string());
    }
    if target.exists() {
        return Err("A file with that name already exists.".to_string());
    }
    Ok(target)
}

#[tauri::command]
fn read_markdown_file(path: String) -> Result<OpenedDocument, String> {
    let path = validate_markdown_path(&path)?;
    let metadata = fs::metadata(&path).map_err(|error| format!("Cannot inspect file: {error}"))?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "The file is larger than the {} MiB Windows MVP limit.",
            MAX_DOCUMENT_BYTES / 1024 / 1024
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("Cannot read file: {error}"))?;
    let (content, format) = decode_document(bytes)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Untitled.md")
        .to_string();
    Ok(OpenedDocument {
        path: path.to_string_lossy().into_owned(),
        name,
        content,
        format: FileFormatDto::from_file_format(format),
    })
}

#[tauri::command]
fn save_markdown_file(path: String, content: String, format: FileFormatDto) -> Result<(), String> {
    let path = validate_markdown_path(&path)?;
    if content.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The document is larger than the 50 MiB Windows MVP limit.".to_string());
    }
    let format = format.to_file_format()?;
    let bytes = format.encode(&content);
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The encoded document would exceed the 50 MiB file limit.".to_string());
    }
    fs::write(&path, bytes).map_err(|error| format!("Cannot save file: {error}"))
}

#[tauri::command]
fn save_html_file(path: String, html: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    if !extension.eq_ignore_ascii_case("html") && !extension.eq_ignore_ascii_case("htm") {
        return Err("The export target must have an .html or .htm extension.".to_string());
    }
    if html.len() > MAX_EXPORT_BYTES {
        return Err("The HTML export is too large.".to_string());
    }
    fs::write(path, html).map_err(|error| format!("Cannot write HTML export: {error}"))
}

#[tauri::command]
fn render_markdown(
    markdown: String,
    title: Option<String>,
    document_path: Option<String>,
    allow_remote_images: Option<bool>,
) -> Result<String, String> {
    if markdown.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The document is larger than the 50 MiB Windows MVP limit.".to_string());
    }

    let title = title.unwrap_or_else(|| "Splat Preview".to_string());
    let doc_dir = document_path
        .as_deref()
        .and_then(|path| Path::new(path).parent().map(Path::to_path_buf));
    let figure = |item: &Figure<'_>| -> Option<String> {
        match item {
            Figure::Math { tex, display } => {
                let key = figure_key(item);
                let display = if *display { "true" } else { "false" };
                Some(format!(
                    "<span class=\"splat-math-figure\" data-figure-key=\"{key}\" data-display=\"{display}\" data-tex=\"{}\">{}</span>",
                    escape_html(tex),
                    escape_html(tex)
                ))
            }
            Figure::Diagram { source, kind } => {
                let key = figure_key(item);
                let kind = diagram_name(*kind);
                Some(format!(
                    "<div class=\"splat-diagram-figure\" data-figure-key=\"{key}\" data-kind=\"{kind}\" data-source=\"{}\"><pre>{}</pre></div>",
                    escape_html(source),
                    escape_html(source)
                ))
            }
        }
    };
    let embed = |path: &str| image_data_url(path, doc_dir.as_deref());
    let rebase = |path: &str| image_data_url(path, doc_dir.as_deref()).unwrap_or_else(|| path.to_string());
    let no_reference = |_reference: &str| None::<String>;
    let no_citation = |_key: &str| None::<String>;

    let export = splat_markdown::html::Export {
        title: &title,
        figure: &figure,
        embed: &embed,
        rebase: &rebase,
        quotes: None,
        reference: &no_reference,
        citation: &no_citation,
    };
    let rendered = splat_markdown::html::to_html(&markdown, &export);
    if allow_remote_images.unwrap_or(false) {
        Ok(rendered)
    } else {
        Ok(rendered.replace(
            "img-src 'self' data: https: file:",
            "img-src data:",
        ))
    }
}

#[tauri::command]
fn export_docx_file(
    path: String,
    markdown: String,
    title: String,
    document_path: Option<String>,
    figures: Vec<FigureAssetDto>,
) -> Result<(), String> {
    let destination = PathBuf::from(path);
    if !destination
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"))
    {
        return Err("The Word export target must have a .docx extension.".to_string());
    }
    if markdown.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The document is larger than the 50 MiB Windows MVP limit.".to_string());
    }

    let mut pictures = HashMap::new();
    for asset in figures {
        let data = BASE64
            .decode(asset.data.as_bytes())
            .map_err(|_| "A rendered figure could not be decoded.".to_string())?;
        if data.len() > 16 * 1024 * 1024 {
            continue;
        }
        if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
            continue;
        }
        let width = if asset.width.is_finite() { asset.width.clamp(12.0, 600.0) } else { 96.0 };
        let height = if asset.height.is_finite() { asset.height.clamp(12.0, 600.0) } else { 48.0 };
        pictures.insert(
            asset.key,
            Picture {
                data,
                jpeg: false,
                width,
                height,
            },
        );
    }

    let doc_dir = document_path
        .as_deref()
        .and_then(|value| Path::new(value).parent().map(Path::to_path_buf));
    let figure = |item: &Figure<'_>| pictures.get(&figure_key(item)).map(|picture| Picture {
        data: picture.data.clone(),
        jpeg: picture.jpeg,
        width: picture.width,
        height: picture.height,
    });
    let image = |url: &str| image_picture(url, doc_dir.as_deref());
    let no_reference = |_reference: &str| None::<String>;
    let no_citation = |_key: &str| None::<String>;
    let title_page = (!title.trim().is_empty()).then(|| TitlePage {
        title: title.clone(),
        ..TitlePage::default()
    });
    let export = WordExport {
        figure: &figure,
        image: &image,
        quotes: None,
        reference: &no_reference,
        citation: &no_citation,
        title: title_page.as_ref(),
    };
    let doc = splat_markdown::reader::parse(&markdown);
    let bytes = splat_markdown::docx::to_docx(&doc, &export)
        .map_err(|_| "The document is too large to fit in a Word file.".to_string())?;
    fs::write(destination, bytes).map_err(|error| format!("Cannot write Word document: {error}"))
}

#[tauri::command]
async fn scan_notes(root: String) -> Result<NoteTreeDto, String> {
    let root = PathBuf::from(root);
    tauri::async_runtime::spawn_blocking(move || {
        if !root.is_dir() {
            return Err("Choose an existing folder to scan for Markdown notes.".to_string());
        }
        let stop = AtomicBool::new(false);
        let tree = splat_notes::scan(&root, &stop);
        Ok(tree_dto(&tree))
    })
    .await
    .map_err(|error| format!("Note scan task failed: {error}"))?
}

#[tauri::command]
async fn search_notes(root: String, query: String, mode: Option<String>) -> Result<NoteSearchDto, String> {
    let root = PathBuf::from(root);
    let mode = mode.unwrap_or_else(|| "text".to_string());
    tauri::async_runtime::spawn_blocking(move || {
        if !root.is_dir() {
            return Err("Choose an existing folder to search.".to_string());
        }
        let stop = AtomicBool::new(false);
        let tree = splat_notes::scan(&root, &stop);
        let wanted = query_for(&mode, &query, &root)?;
        let mut files = Vec::new();
        let searched = splat_notes::search(&tree, &wanted, &stop, &mut |file| files.push(file));
        Ok(search_dto(files, searched.unreadable))
    })
    .await
    .map_err(|error| format!("Note search task failed: {error}"))?
}

#[tauri::command]
async fn rename_note(path: String, new_name: String) -> Result<RenameResultDto, String> {
    let from = validate_markdown_path(&path)?;
    let to = new_note_path(&from, &new_name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = from
            .parent()
            .ok_or_else(|| "The note has no parent folder.".to_string())?
            .to_path_buf();
        let stop = AtomicBool::new(false);
        let tree = splat_notes::scan(&root, &stop);
        let mut decoded: HashMap<PathBuf, (String, FileFormat)> = HashMap::new();
        let rewrites: Vec<Rewrite> = splat_notes::renames(&tree, &from, &to, &stop, &mut |note| {
            let bytes = fs::read(note).ok()?;
            let (text, format) = decode_document(bytes).ok()?;
            decoded.insert(note.to_path_buf(), (text.clone(), format));
            Some(text)
        });

        let mut staged = Vec::new();
        for rewrite in &rewrites {
            let (text, format) = if let Some(decoded) = decoded.get(&rewrite.path) {
                (decoded.0.clone(), decoded.1.clone())
            } else {
                let bytes = fs::read(&rewrite.path)
                    .map_err(|error| format!("Cannot read {}: {error}", rewrite.path.display()))?;
                decode_document(bytes)?
            };
            let updated = splat_notes::applied(&text, &rewrite.replacements);
            if updated == text {
                continue;
            }
            let original = fs::read(&rewrite.path)
                .map_err(|error| format!("Cannot back up {}: {error}", rewrite.path.display()))?;
            staged.push((rewrite.path.clone(), original, format.encode(&updated)));
        }

        let mut written: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for (file, backup, updated) in &staged {
            if let Err(error) = fs::write(file, updated) {
                for (changed, original) in written.iter().rev() {
                    let _ = fs::write(changed, original);
                }
                return Err(format!("Cannot update link in {}: {error}", file.display()));
            }
            written.push((file.clone(), backup.clone()));
        }

        if let Err(error) = fs::rename(&from, &to) {
            for (changed, original) in written.iter().rev() {
                let _ = fs::write(changed, original);
            }
            return Err(format!("Cannot rename note: {error}"));
        }

        Ok(RenameResultDto {
            old_path: from.to_string_lossy().into_owned(),
            new_path: to.to_string_lossy().into_owned(),
            rewritten_files: staged.len(),
        })
    })
    .await
    .map_err(|error| format!("Note rename task failed: {error}"))?
}

#[tauri::command]
fn git_changes(path: String) -> Result<Vec<GitChange>, String> {
    let file = PathBuf::from(path);
    let Some(repository) = splat_git::find(&file) else {
        return Ok(Vec::new());
    };
    let Some(previous) = repository
        .head_version(&file)
        .map_err(|error| format!("Cannot read Git version: {error}"))?
    else {
        return Ok(Vec::new());
    };
    let current_bytes = fs::read(&file).map_err(|error| format!("Cannot read current file: {error}"))?;
    let (current, _) = decode_document(current_bytes)?;
    let (previous, _) = decode_document(previous)
        .map_err(|_| "The committed Markdown file is not valid UTF-8 or BOM-marked UTF-16.".to_string())?;

    Ok(splat_git::diff::changes(&previous, &current)
        .into_iter()
        .map(|change| {
            let kind = match change.kind {
                splat_git::diff::ChangeKind::Added => "added",
                splat_git::diff::ChangeKind::Modified => "modified",
                splat_git::diff::ChangeKind::Removed => "removed",
            };
            GitChange {
                kind,
                start_line: change.lines.start + 1,
                end_line: change.lines.end,
                text: current.get(change.bytes).unwrap_or_default().to_string(),
            }
        })
        .collect())
}

#[tauri::command]
fn paste_html_as_markdown(html: String) -> Result<String, String> {
    let pasted = splat_import::Pasted {
        html: Some(&html),
        rtf: None,
        plain: None,
    };
    let converted = splat_import::paste_as_markdown(&pasted, &mut |_image| None)
        .ok_or_else(|| "The clipboard HTML did not contain convertible content.".to_string())?;
    Ok(converted.markdown)
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            read_markdown_file,
            save_markdown_file,
            save_html_file,
            export_docx_file,
            render_markdown,
            scan_notes,
            search_notes,
            rename_note,
            git_changes,
            paste_html_as_markdown,
            save_recovery_snapshot,
            load_recovery_snapshot,
            clear_recovery_snapshot,
            startup_file,
            app_version,
        ])
        .setup(|app| {
            let menu = MenuBuilder::new(app)
                .text("new", "New")
                .text("open", "Open…")
                .text("save", "Save")
                .text("save-as", "Save As…")
                .separator()
                .text("export-html", "Export HTML…")
                .text("export-word", "Export Word…")
                .text("export-pdf", "Print / Export PDF…")
                .separator()
                .text("notes", "Folder Notes")
                .text("search", "Search Notes")
                .separator()
                .text("check-updates", "Check for Updates")
                .text("about", "About Splat")
                .build()?;
            app.set_menu(menu)?;
            app.on_menu_event(|handle, event| {
                let _ = handle.emit("splat-menu", event.id().0.clone());
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Splat for Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_extensions_are_case_insensitive() {
        assert!(is_markdown_path(Path::new("notes.md")));
        assert!(is_markdown_path(Path::new("notes.MARKDOWN")));
        assert!(is_markdown_path(Path::new("notes.mdown")));
        assert!(is_markdown_path(Path::new("notes.mkd")));
        assert!(!is_markdown_path(Path::new("notes.txt")));
        assert!(!is_markdown_path(Path::new("notes")));
    }

    #[test]
    fn utf8_crlf_and_bom_survive_a_round_trip() {
        let source = b"\xEF\xBB\xBF# Title\r\n\r\nText\r\n".to_vec();
        let (text, format) = decode_document(source.clone()).expect("valid UTF-8");
        assert_eq!(text, "# Title\r\n\r\nText\r\n");
        assert_eq!(format.encode(&text), source);
    }

    #[test]
    fn utf16le_survives_a_round_trip() {
        let source = b"\xFF\xFE#\x00 \x00T\x00\r\x00\n\x00".to_vec();
        let (text, format) = decode_document(source.clone()).expect("valid UTF-16LE");
        assert_eq!(text, "# T\r\n");
        assert!(!format.bom, "UTF-16 encoders write their required BOM themselves");
        assert_eq!(format.encode(&text), source);
    }

    #[test]
    fn utf16be_survives_a_round_trip() {
        let source = b"\xFE\xFF\x00#\x00 \x00T\x00\r\x00\n".to_vec();
        let (text, format) = decode_document(source.clone()).expect("valid UTF-16BE");
        assert_eq!(text, "# T\r\n");
        assert_eq!(format.encode(&text), source);
    }

    #[test]
    fn format_dto_rejects_unknown_encodings() {
        let format = FileFormatDto {
            bom: false,
            crlf: false,
            encoding: "unknown".to_string(),
        };
        assert!(format.to_file_format().is_err());
    }

    #[test]
    fn figure_keys_distinguish_formula_mode_and_diagram_language() {
        let inline = Figure::Math { tex: "x^2", display: false };
        let display = Figure::Math { tex: "x^2", display: true };
        let mermaid = Figure::Diagram { source: "A-->B", kind: DiagramKind::Mermaid };
        let graphviz = Figure::Diagram { source: "A-->B", kind: DiagramKind::Graphviz };
        assert_ne!(figure_key(&inline), figure_key(&display));
        assert_ne!(figure_key(&mermaid), figure_key(&graphviz));
    }

    #[test]
    fn note_rename_rejects_paths_and_keeps_markdown_extension() {
        let from = Path::new("notes/old.markdown");
        assert_eq!(new_note_path(from, "new").expect("valid name"), Path::new("notes/new.markdown"));
        assert!(new_note_path(from, "../outside").is_err());
        assert!(new_note_path(from, "nested/name").is_err());
    }

    #[test]
    fn html_escaping_protects_attributes_and_source_blocks() {
        assert_eq!(escape_html("<x a=\"&\">'"), "&lt;x a=&quot;&amp;&quot;&gt;&#39;");
    }

    #[test]
    fn local_path_rejects_traversal_and_urls() {
        assert!(!is_safe_relative_path("../secret.png"));
        assert!(!is_safe_relative_path("https://example.com/image.png"));
        assert!(is_safe_relative_path("images/photo.png"));
    }
}
