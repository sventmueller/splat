use serde::{Deserialize, Serialize};
use splat_file_state::{Encoding, FileFormat};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_DOCUMENT_BYTES: u64 = 50 * 1024 * 1024;

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

#[tauri::command]
fn read_markdown_file(path: String) -> Result<OpenedDocument, String> {
    let path = validate_markdown_path(&path)?;
    let metadata = fs::metadata(&path).map_err(|error| format!("Cannot inspect file: {error}"))?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("The file is larger than the {} MiB Windows MVP limit.", MAX_DOCUMENT_BYTES / 1024 / 1024));
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
    if html.len() as u64 > MAX_DOCUMENT_BYTES * 2 {
        return Err("The HTML export is too large.".to_string());
    }
    fs::write(path, html).map_err(|error| format!("Cannot write HTML export: {error}"))
}

#[tauri::command]
fn render_markdown(
    markdown: String,
    title: Option<String>,
    allow_remote_images: Option<bool>,
) -> Result<String, String> {
    if markdown.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("The document is larger than the 50 MiB Windows MVP limit.".to_string());
    }

    // The platform-independent Splat renderer sanitizes embedded HTML, validates link schemes,
    // and emits a restrictive Content Security Policy for the preview document.
    let title = title.unwrap_or_else(|| "Splat Preview".to_string());
    let no_figure = |_figure: &splat_markdown::doc::Figure| None::<String>;
    let no_embed = |_path: &str| None::<String>;
    let keep_relative = |path: &str| path.to_string();
    let no_reference = |_reference: &str| None::<String>;
    let no_citation = |_key: &str| None::<String>;

    let export = splat_markdown::html::Export {
        title: &title,
        figure: &no_figure,
        embed: &no_embed,
        rebase: &keep_relative,
        quotes: None,
        reference: &no_reference,
        citation: &no_citation,
    };
    let rendered = splat_markdown::html::to_html(&markdown, &export);
    if allow_remote_images.unwrap_or(false) {
        Ok(rendered)
    } else {
        // The macOS shell has a permission flow for remote images. Until the Windows shell grows
        // the same consent UI, keep the live preview fully offline and avoid file:// image access.
        Ok(rendered.replace(
            "img-src 'self' data: https: file:",
            "img-src data:",
        ))
    }
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
            render_markdown,
            git_changes,
            paste_html_as_markdown,
            app_version,
        ])
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
}
