//! The text of one Word file in the look of another: all parts of the template stay, only the content of its main
//! document's body changes, and what the generated text needs is added to its parts. A template in the Strict form
//! comes out in the transitional one that the generated text is written in.

mod notes;
mod numbering;
mod package;
mod serialize;
mod styles;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fmt;

use roxmltree::{Document, Node};
use splat_markdown::docx::TooLarge;
use splat_markdown::docx::zip::Zip;

use crate::package::{PackageError, Relationships, TYPES_PART};
use crate::xml::{self, XmlError};
use crate::zip::ZipError;
use notes::{KINDS, NoteKind};
use package::{PartText, RelsPart, Word, merged_types, target_from};
use serialize::{Edits, Names, RELATIONSHIPS, Rewrite, WORD, is_word, standalone, with_children_added, write_tree};

/// Why a template cannot be applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateError {
    /// One of the two files is of another kind, a Word file with macros among them.
    NotAWordFile,
    /// One of the two files is protected by a password.
    Encrypted,
    /// One of the two files, or what they make together, is larger than a merge may hold.
    TooLarge,
    /// One of the two files has a broken structure.
    Damaged,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TemplateError::NotAWordFile => "not a Word document or template without macros",
            TemplateError::Encrypted => "a Word file protected by a password",
            TemplateError::TooLarge => "larger than a Word template may be applied to",
            TemplateError::Damaged => "a damaged Word file",
        })
    }
}

impl std::error::Error for TemplateError {}

impl From<PackageError> for TemplateError {
    fn from(error: PackageError) -> TemplateError {
        match error {
            PackageError::NotAWordFile => TemplateError::NotAWordFile,
            PackageError::Encrypted => TemplateError::Encrypted,
            PackageError::TooLarge => TemplateError::TooLarge,
            PackageError::Damaged => TemplateError::Damaged,
        }
    }
}

impl From<ZipError> for TemplateError {
    fn from(error: ZipError) -> TemplateError {
        PackageError::from(error).into()
    }
}

impl From<XmlError> for TemplateError {
    fn from(error: XmlError) -> TemplateError {
        PackageError::from(error).into()
    }
}

impl From<TooLarge> for TemplateError {
    fn from(_: TooLarge) -> TemplateError {
        TemplateError::TooLarge
    }
}

/// A valid .docx with the text of `generated`, which `to_docx` wrote, in the look of `template`, a .docx or .dotx.
pub fn apply_template(generated: &[u8], template: &[u8]) -> Result<Vec<u8>, TemplateError> {
    let generated = Word::open(generated)?;
    let template = Word::open(template)?;
    let texts = Texts::read(&generated, &template)?;
    let parts = Parts::parse(&texts)?;
    let mut merge = Merge::new(&generated, &template);
    merge.merge_all(&parts)?;
    merge.package()
}

/// What is read of both files as a whole.
struct Texts {
    generated_document: String,
    template_document: String,
    generated_styles: Option<PartText>,
    template_styles: Option<PartText>,
    generated_numbering: Option<PartText>,
    template_numbering: Option<PartText>,
    generated_notes: Vec<Option<PartText>>,
    template_notes: Vec<Option<PartText>>,
}

impl Texts {
    fn read(generated: &Word, template: &Word) -> Result<Texts, TemplateError> {
        let notes = |word: &Word| -> Result<Vec<Option<PartText>>, TemplateError> {
            KINDS.iter().map(|kind| word.related(kind.suffix)).collect()
        };
        Ok(Texts {
            generated_document: generated
                .package
                .text(&generated.package.main)?
                .ok_or(TemplateError::Damaged)?,
            template_document: template
                .package
                .text(&template.package.main)?
                .ok_or(TemplateError::Damaged)?,
            generated_styles: generated.related("styles")?,
            template_styles: template.related("styles")?,
            generated_numbering: generated.related("numbering")?,
            template_numbering: template.related("numbering")?,
            generated_notes: notes(generated)?,
            template_notes: notes(template)?,
        })
    }
}

/// A part read as XML.
struct Part<'t> {
    source: &'t PartText,
    doc: Document<'t>,
}

fn parse_part(part: Option<&PartText>) -> Result<Option<Part<'_>>, TemplateError> {
    let Some(source) = part else {
        return Ok(None);
    };
    Ok(Some(Part {
        source,
        doc: xml::parse(&source.text)?,
    }))
}

/// What the merge works on.
struct Parts<'t> {
    generated_document: Document<'t>,
    template_document: Document<'t>,
    template_text: &'t str,
    generated_styles: Option<Part<'t>>,
    template_styles: Option<Part<'t>>,
    generated_numbering: Option<Part<'t>>,
    template_numbering: Option<Part<'t>>,
    generated_notes: Vec<Option<Part<'t>>>,
    template_notes: Vec<Option<Part<'t>>>,
}

impl<'t> Parts<'t> {
    fn parse(texts: &'t Texts) -> Result<Parts<'t>, TemplateError> {
        let notes = |parts: &'t [Option<PartText>]| -> Result<Vec<Option<Part<'t>>>, TemplateError> {
            parts.iter().map(|part| parse_part(part.as_ref())).collect()
        };
        Ok(Parts {
            generated_document: xml::parse(&texts.generated_document)?,
            template_document: xml::parse(&texts.template_document)?,
            template_text: &texts.template_document,
            generated_styles: parse_part(texts.generated_styles.as_ref())?,
            template_styles: parse_part(texts.template_styles.as_ref())?,
            generated_numbering: parse_part(texts.generated_numbering.as_ref())?,
            template_numbering: parse_part(texts.template_numbering.as_ref())?,
            generated_notes: notes(&texts.generated_notes)?,
            template_notes: notes(&texts.template_notes)?,
        })
    }
}

/// The body of a main document.
fn body_of<'a, 'i>(doc: &'a Document<'i>) -> Result<Node<'a, 'i>, TemplateError> {
    let root = doc.root_element();
    if !is_word(&root, "document") {
        return Err(TemplateError::Damaged);
    }
    root.children()
        .find(|node| is_word(node, "body"))
        .ok_or(TemplateError::Damaged)
}

/// The final section properties of a body.
fn final_section<'a, 'i>(body: &Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    body.last_element_child().filter(|node| is_word(node, "sectPr"))
}

/// Things in the order they are first met, each once.
#[derive(Default)]
struct InOrder {
    list: Vec<String>,
    seen: HashSet<String>,
}

impl InOrder {
    fn add(&mut self, value: &str) {
        if self.seen.insert(value.to_string()) {
            self.list.push(value.to_string());
        }
    }
}

/// Takes in the style ids `node` and what it holds use.
fn collect_styles(node: Node, used: &mut InOrder) {
    for element in node.descendants().filter(Node::is_element) {
        if ["pStyle", "rStyle", "tblStyle"]
            .iter()
            .any(|name| is_word(&element, name))
            && let Some(id) = element.attribute((WORD, "val"))
        {
            used.add(id);
        }
    }
}

/// The relationships `nodes` and what they hold refer to.
fn collect_relationships<'a, 'i>(nodes: impl Iterator<Item = Node<'a, 'i>>) -> Vec<String>
where
    'i: 'a,
{
    let mut used = InOrder::default();
    for node in nodes {
        for element in node.descendants().filter(Node::is_element) {
            for attribute in element.attributes().filter(|a| a.namespace() == Some(RELATIONSHIPS)) {
                used.add(attribute.value());
            }
        }
    }
    used.list
}

fn holds(node: Node, name: &str) -> bool {
    node.descendants().any(|element| is_word(&element, name))
}

/// The merge under way: what was already decided, and what is left to write.
struct Merge<'w, 'a> {
    generated: &'w Word<'a>,
    template: &'w Word<'a>,
    /// New content of the template's parts.
    changed: HashMap<String, Vec<u8>>,
    new_parts: Vec<(String, Vec<u8>)>,
    /// The names of all parts in lowercase, as parts differ by more than case in a package.
    taken: HashSet<String>,
    /// Pictures copied, by their name in the generated file.
    media: HashMap<String, String>,
    new_defaults: Vec<(String, String)>,
    known_extensions: HashSet<String>,
    new_overrides: Vec<(String, String)>,
    rewrite: Rewrite,
}

impl<'w, 'a> Merge<'w, 'a> {
    fn new(generated: &'w Word<'a>, template: &'w Word<'a>) -> Merge<'w, 'a> {
        Merge {
            generated,
            template,
            changed: HashMap::new(),
            new_parts: Vec::new(),
            taken: template.package.archive.names().map(str::to_lowercase).collect(),
            media: HashMap::new(),
            new_defaults: Vec::new(),
            known_extensions: template.package.defaults.keys().cloned().collect(),
            new_overrides: Vec::new(),
            rewrite: Rewrite::default(),
        }
    }

    fn merge_all(&mut self, parts: &Parts) -> Result<(), TemplateError> {
        let body = body_of(&parts.generated_document)?;
        let template_body = body_of(&parts.template_document)?;
        let used_notes: Vec<Option<&Part>> = KINDS
            .iter()
            .zip(&parts.generated_notes)
            .map(|(kind, part)| part.as_ref().filter(|_| holds(body, kind.reference)))
            .collect();
        let mut used_styles = InOrder::default();
        collect_styles(body, &mut used_styles);
        let mut lists = holds(body, "numId");
        for part in used_notes.iter().flatten() {
            collect_styles(part.doc.root_element(), &mut used_styles);
            lists |= holds(part.doc.root_element(), "numId");
        }
        if let Some(part) = parts.generated_numbering.as_ref().filter(|_| lists) {
            collect_styles(part.doc.root_element(), &mut used_styles);
        }

        let style_plan = match (&parts.generated_styles, &parts.template_styles) {
            (Some(generated), Some(template)) => Some(styles::plan(&used_styles.list, &generated.doc, &template.doc)),
            _ => None,
        };
        lists |= match (&style_plan, &parts.generated_styles) {
            (Some(plan), _) => plan.added.iter().any(|style| holds(*style, "numId")),
            (None, Some(generated)) => holds(generated.doc.root_element(), "numId"),
            (None, None) => false,
        };
        let numbering_source = parts.generated_numbering.as_ref().filter(|_| lists);
        let number_plan = match (numbering_source, &parts.template_numbering) {
            (Some(generated), Some(template)) => Some(numbering::plan(&generated.doc, &template.doc)),
            _ => None,
        };
        let note_plans: Vec<Option<notes::NotePlan>> = KINDS
            .iter()
            .enumerate()
            .map(|(at, kind)| {
                let template = parts.template_notes[at].as_ref().map(|part| &part.doc);
                used_notes[at].map(|part| notes::plan(&part.doc, template, *kind))
            })
            .collect();
        self.set_maps(style_plan.as_ref(), number_plan.as_ref(), &note_plans);

        let mut rels = RelsPart::of(self.template, &self.template.package.main)?;
        match (&style_plan, &parts.generated_styles, &parts.template_styles) {
            (Some(plan), _, Some(template)) if !plan.added.is_empty() => {
                let root = template.doc.root_element();
                self.rewrite.adding_styles = true;
                let text = with_children_added(&template.source.text, root, &plan.added, &self.rewrite);
                self.rewrite.adding_styles = false;
                self.put(template.source.name.clone(), text.into_bytes());
            }
            (None, Some(generated), _) => self.take_along(generated, "styles", &mut rels),
            _ => {}
        }
        match (&number_plan, numbering_source, &parts.template_numbering) {
            (Some(plan), _, Some(template)) => {
                let root = template.doc.root_element();
                let text = numbering::merged(&template.source.text, root, plan, &self.rewrite);
                self.put(template.source.name.clone(), text.into_bytes());
            }
            (None, Some(generated), _) => self.take_along(generated, "numbering", &mut rels),
            _ => {}
        }
        for (at, plan) in note_plans.iter().enumerate() {
            if let (Some(plan), Some(part)) = (plan, used_notes[at]) {
                self.merge_note_part(part, parts.template_notes[at].as_ref(), plan, KINDS[at], &mut rels)?;
            }
        }

        let referred = collect_relationships(body.children());
        let generated = self.generated;
        self.rewrite.relationships = self.map_relationships(&generated.rels, &referred, &mut rels)?;
        let document = self.document(parts, body, template_body);
        self.put(self.template.package.main.clone(), document.into_bytes());
        self.put_rels(rels)
    }

    /// Says how ids read in the template before anything is written, as parts refer to each other.
    fn set_maps(
        &mut self,
        styles: Option<&styles::StylePlan>,
        numbers: Option<&numbering::NumberPlan>,
        notes: &[Option<notes::NotePlan>],
    ) {
        if let Some(plan) = styles {
            self.rewrite.styles.clone_from(&plan.map);
        }
        if let Some(plan) = numbers {
            self.rewrite.abstract_numbers.clone_from(&plan.abstract_map);
            self.rewrite.numbers.clone_from(&plan.number_map);
        }
        let map_of = |at: usize| notes[at].as_ref().map(|plan| plan.map.clone()).unwrap_or_default();
        self.rewrite.footnotes = map_of(0);
        self.rewrite.endnotes = map_of(1);
    }

    fn merge_note_part(
        &mut self,
        generated: &Part,
        template: Option<&Part>,
        plan: &notes::NotePlan,
        kind: NoteKind,
        rels: &mut RelsPart,
    ) -> Result<(), TemplateError> {
        let source_rels = self.generated.package.relationships(&generated.source.name)?;
        let Some(template) = template else {
            let new_name = self.unique_name("word", kind.stem, "xml");
            let mut own_rels = RelsPart::of(self.template, &new_name)?;
            let root = generated.doc.root_element();
            let referred = collect_relationships(std::iter::once(root));
            self.rewrite.relationships = self.map_relationships(&source_rels, &referred, &mut own_rels)?;
            self.take_along_as(generated, &new_name, kind.stem, rels);
            self.put_rels(own_rels)?;
            return Ok(());
        };
        let mut own_rels = RelsPart::of(self.template, &template.source.name)?;
        let referred = collect_relationships(plan.added.iter().copied());
        self.rewrite.relationships = self.map_relationships(&source_rels, &referred, &mut own_rels)?;
        let text = with_children_added(
            &template.source.text,
            template.doc.root_element(),
            &plan.added,
            &self.rewrite,
        );
        self.put(template.source.name.clone(), text.into_bytes());
        self.put_rels(own_rels)
    }

    /// Takes a part of the generated file along under a name of its own.
    fn take_along(&mut self, part: &Part, stem: &str, rels: &mut RelsPart) {
        let name = self.unique_name("word", stem, "xml");
        self.take_along_as(part, &name, stem, rels);
    }

    fn take_along_as(&mut self, part: &Part, name: &str, stem: &str, rels: &mut RelsPart) {
        let mut text = String::new();
        standalone(part.doc.root_element(), &mut text, &self.rewrite);
        self.put(name.to_string(), text.into_bytes());
        let kind = self
            .generated
            .package
            .overrides
            .get(&part.source.name)
            .cloned()
            .unwrap_or_else(|| format!("application/vnd.openxmlformats-officedocument.wordprocessingml.{stem}+xml"));
        self.new_overrides.push((name.to_string(), kind));
        let target = target_from(&rels.folder, name);
        rels.add(&part.source.kind, &target, false);
    }

    /// New ids in `target` for the pictures and links in `used`; a relationship to anything else or to nothing is left out.
    fn map_relationships(
        &mut self,
        source: &Relationships,
        used: &[String],
        target: &mut RelsPart,
    ) -> Result<HashMap<String, String>, TemplateError> {
        let mut map = HashMap::new();
        for id in used {
            let Some(rel) = source.get(id) else {
                continue;
            };
            if !rel.kind.ends_with("/image") && !rel.kind.ends_with("/hyperlink") {
                continue;
            }
            let new_id = if rel.external {
                target.add(&rel.kind, &rel.target, true)
            } else {
                let copied = match &rel.part {
                    Some(part) => self.copy_media(part)?,
                    None => None,
                };
                let Some(copied) = copied else {
                    continue;
                };
                let relative = target_from(&target.folder, &copied);
                target.add(&rel.kind, &relative, false)
            };
            map.insert(id.clone(), new_id);
        }
        Ok(map)
    }

    /// Copies a picture once; its name in the template comes from the file name alone, so no name leads out of the media
    /// folder, whatever the generated file says.
    fn copy_media(&mut self, source: &str) -> Result<Option<String>, TemplateError> {
        if let Some(done) = self.media.get(source) {
            return Ok(Some(done.clone()));
        }
        let Some(bytes) = self.generated.package.archive.get(source)? else {
            return Ok(None);
        };
        let file = source.rsplit('/').next().unwrap_or(source);
        let (stem, extension) = file.rsplit_once('.').unwrap_or((file, ""));
        let extension = if !extension.is_empty() && extension.bytes().all(|b| b.is_ascii_alphanumeric()) {
            extension.to_ascii_lowercase()
        } else {
            "bin".to_string()
        };
        let stem: String = stem
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        let name = self.unique_name("word/media", if stem.is_empty() { "image" } else { &stem }, &extension);
        if self.known_extensions.insert(extension.clone()) {
            let kind = self
                .generated
                .package
                .defaults
                .get(&extension)
                .map_or_else(|| media_type(&extension).to_string(), Clone::clone);
            self.new_defaults.push((extension, kind));
        }
        self.media.insert(source.to_string(), name.clone());
        self.new_parts.push((name.clone(), bytes));
        Ok(Some(name))
    }

    fn unique_name(&mut self, folder: &str, stem: &str, extension: &str) -> String {
        let plain = format!("{folder}/{stem}.{extension}");
        let name = if self.taken.contains(&plain.to_lowercase()) {
            (1..)
                .map(|n| format!("{folder}/{stem}_{n}.{extension}"))
                .find(|name| !self.taken.contains(&name.to_lowercase()))
                .unwrap_or(plain)
        } else {
            plain
        };
        self.taken.insert(name.to_lowercase());
        name
    }

    fn put(&mut self, name: String, content: Vec<u8>) {
        if self.template.package.archive.contains(&name) {
            self.changed.insert(name, content);
        } else {
            self.new_parts.push((name, content));
        }
    }

    fn put_rels(&mut self, rels: RelsPart) -> Result<(), TemplateError> {
        if let Some((name, text)) = rels.finish()? {
            if !self.template.package.archive.contains(&name) && self.known_extensions.insert("rels".to_string()) {
                self.new_defaults.push((
                    "rels".to_string(),
                    "application/vnd.openxmlformats-package.relationships+xml".to_string(),
                ));
            }
            self.put(name, text.into_bytes());
        }
        Ok(())
    }

    /// The template's main document with the generated body in place of what its body held.
    fn document(&self, parts: &Parts, body: Node, template_body: Node) -> String {
        let text = parts.template_text;
        let section = final_section(&template_body);
        let generated_section = final_section(&body);
        let mut names = Names::of(template_body);
        let mut content = String::new();
        for child in body.children().filter(|child| Some(*child) != generated_section) {
            write_tree(child, &mut names, &self.rewrite, &mut content);
        }
        match (section, generated_section) {
            (Some(section), _) => content.push_str(text.get(section.range()).unwrap_or("")),
            (None, Some(section)) => write_tree(section, &mut names, &self.rewrite, &mut content),
            (None, None) => {}
        }
        let mut edits = Edits::new(text);
        let root = parts.template_document.root_element();
        match (template_body.first_child(), template_body.last_child()) {
            (Some(first), Some(last)) => {
                edits.insert(edits.behind_name(root), names.declarations());
                edits.replace(first.range().start..last.range().end, content);
            }
            _ => {
                let mut name = String::new();
                names.write_name(Some(WORD), "body", true, template_body, &mut name);
                edits.insert(edits.behind_name(root), names.declarations());
                edits.replace(template_body.range(), format!("<{name}>{content}</{name}>"));
            }
        }
        edits.finish()
    }

    fn package(self) -> Result<Vec<u8>, TemplateError> {
        let types = merged_types(
            &self.template.package.types,
            &self.template.package.main,
            &self.new_defaults,
            &self.new_overrides,
        )?;
        let mut zip = Zip::default();
        zip.add(TYPES_PART, types.as_bytes())?;
        for name in self.template.package.archive.names() {
            if name == TYPES_PART || name.ends_with('/') {
                continue;
            }
            match self.changed.get(name) {
                Some(content) => zip.add(name, content)?,
                None => zip.add(name, &self.template.package.part(name)?.unwrap_or_default())?,
            }
        }
        for (name, content) in &self.new_parts {
            zip.add(name, content)?;
        }
        Ok(zip.finish())
    }
}

/// What a picture's extension says if the generated file does not.
fn media_type(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => "application/octet-stream",
    }
}
