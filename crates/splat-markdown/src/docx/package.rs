//! The fixed parts of a Word file, around its text.

pub(super) fn numbering(ordered: &[usize]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    );
    for (id, decimal) in [(0, false), (1, true)] {
        out.push_str(&format!(
            "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
        ));
        for level in 0..9 {
            let (format, text) = if decimal {
                ("decimal", format!("%{}.", level + 1))
            } else {
                ("bullet", ["•", "◦", "▪"][level % 3].to_string())
            };
            out.push_str(&format!(
                "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{text}\"/>\
                 <w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                720 * (level + 1)
            ));
        }
        out.push_str("</w:abstractNum>");
    }
    out.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for (n, start) in ordered.iter().enumerate() {
        out.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            n + 2
        ));
    }
    out.push_str("</w:numbering>");
    out
}

pub(super) const NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
    xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

pub(super) const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
<Override PartName=\"/word/fontTable.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
</Types>";

pub(super) const PACKAGE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

pub(super) const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Helvetica Neue\" w:hAnsi=\"Helvetica Neue\" w:eastAsia=\"Helvetica Neue\" w:cs=\"Helvetica Neue\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"280\" w:after=\"100\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"30\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"26\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"3\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"24\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"4\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"5\"/></w:pPr><w:rPr><w:b/><w:i/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:before=\"3600\" w:after=\"240\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"56\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Subtitle\"><w:name w:val=\"Subtitle\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"120\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:color w:val=\"555555\"/><w:sz w:val=\"28\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"8\" w:color=\"D2D2D7\"/></w:pBdr><w:ind w:left=\"567\"/></w:pPr><w:rPr><w:color w:val=\"555555\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F5\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Menlo\" w:hAnsi=\"Menlo\" w:cs=\"Menlo\"/><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"CodeChar\"><w:name w:val=\"Code Char\"/><w:rPr><w:rFonts w:ascii=\"Menlo\" w:hAnsi=\"Menlo\" w:cs=\"Menlo\"/><w:sz w:val=\"19\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F5\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:left w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/>\
<w:right w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/>\
</w:tblBorders><w:tblCellMar><w:left w:w=\"108\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>\
</w:styles>";

/// Without a font, Word sets Times New Roman. Helvetica Neue comes with every Mac; where it is missing, Word picks
/// a substitute by the family given here, a sans serif like Arial.
pub(super) const FONT_TABLE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:font w:name=\"Helvetica Neue\"><w:altName w:val=\"Arial\"/><w:family w:val=\"swiss\"/><w:pitch w:val=\"variable\"/></w:font>\
<w:font w:name=\"Menlo\"><w:altName w:val=\"Consolas\"/><w:family w:val=\"modern\"/><w:pitch w:val=\"fixed\"/></w:font>\
</w:fonts>";

/// Without a compatibility mode Word opens the document in the mode of Word 2007 and says so in the title bar.
pub(super) const SETTINGS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:compat>\
<w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/>\
</w:compat></w:settings>";
