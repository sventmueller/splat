//! Word's formulas in RTF spell OMML's elements as control words, `{\mf{\mnum …}{\mden …}}` for `<m:f>`; they are turned
//! back into OMML for the reader of Word files to make LaTeX of.

use splat_ooxml::xml::escape_text;

/// OMML's elements and properties, which a group's first control word names with an `m` in front.
const ELEMENTS: &[&str] = &[
    "acc",
    "accPr",
    "aln",
    "alnScr",
    "argPr",
    "argSz",
    "bar",
    "barPr",
    "baseJc",
    "begChr",
    "borderBox",
    "borderBoxPr",
    "box",
    "boxPr",
    "brk",
    "cGp",
    "cGpRule",
    "chr",
    "count",
    "cSp",
    "ctrlPr",
    "d",
    "deg",
    "degHide",
    "den",
    "diff",
    "dPr",
    "e",
    "endChr",
    "eqArr",
    "eqArrPr",
    "f",
    "fName",
    "fPr",
    "func",
    "funcPr",
    "groupChr",
    "groupChrPr",
    "grow",
    "hideBot",
    "hideLeft",
    "hideRight",
    "hideTop",
    "lim",
    "limLoc",
    "limLow",
    "limLowPr",
    "limUpp",
    "limUppPr",
    "lit",
    "m",
    "maxDist",
    "mc",
    "mcJc",
    "mcPr",
    "mcs",
    "mPr",
    "mr",
    "nary",
    "naryPr",
    "noBreak",
    "nor",
    "num",
    "objDist",
    "oMath",
    "oMathPara",
    "oMathParaPr",
    "opEmu",
    "phant",
    "phantPr",
    "plcHide",
    "pos",
    "r",
    "rad",
    "radPr",
    "rSp",
    "rSpRule",
    "scr",
    "sepChr",
    "show",
    "shp",
    "sPre",
    "sPrePr",
    "sSub",
    "sSubPr",
    "sSubSup",
    "sSubSupPr",
    "sSup",
    "sSupPr",
    "strikeBLTR",
    "strikeH",
    "strikeTLBR",
    "strikeV",
    "sty",
    "sub",
    "subHide",
    "sup",
    "supHide",
    "transp",
    "type",
    "vertJc",
    "zeroAsc",
    "zeroDesc",
    "zeroWid",
];

/// The elements whose text is their `m:val`.
const VALUED: &[&str] = &[
    "alnScr",
    "argSz",
    "baseJc",
    "begChr",
    "cGp",
    "cGpRule",
    "chr",
    "count",
    "cSp",
    "degHide",
    "diff",
    "endChr",
    "grow",
    "hideBot",
    "hideLeft",
    "hideRight",
    "hideTop",
    "limLoc",
    "lit",
    "maxDist",
    "mcJc",
    "noBreak",
    "nor",
    "objDist",
    "opEmu",
    "plcHide",
    "pos",
    "rSp",
    "rSpRule",
    "scr",
    "sepChr",
    "show",
    "shp",
    "strikeBLTR",
    "strikeH",
    "strikeTLBR",
    "strikeV",
    "sty",
    "subHide",
    "supHide",
    "transp",
    "type",
    "vertJc",
    "zeroAsc",
    "zeroDesc",
    "zeroWid",
];

/// A group of the formula and what it has made so far.
#[derive(Default)]
struct Group {
    element: Option<&'static str>,
    xml: String,
    /// Text not yet written: a run's or a value's, or text that stands where OMML has only elements.
    text: String,
    /// `\msty0`: the run is set upright, which the reader of Word files takes for a word such as `sin`.
    upright: bool,
    /// `\mnor`: the run is text, not mathematics.
    normal: bool,
    /// The group holds no part of the formula, as the picture Word puts beside it for readers that cannot show it.
    skipped: bool,
}

impl Group {
    fn holds_text(&self) -> bool {
        self.element
            .is_some_and(|element| element == "r" || VALUED.contains(&element))
    }

    /// Text that stands among elements becomes a run of its own; spaces there are none.
    fn flush(&mut self) {
        if self.holds_text() {
            return;
        }
        if !self.text.trim().is_empty() {
            let text = std::mem::take(&mut self.text);
            write_run(&text, false, false, &mut self.xml);
        }
        self.text.clear();
    }
}

fn write_run(text: &str, upright: bool, normal: bool, out: &mut String) {
    out.push_str("<m:r>");
    if upright || normal {
        out.push_str("<m:rPr>");
        if upright {
            out.push_str("<m:sty m:val=\"p\"/>");
        }
        if normal {
            out.push_str("<m:nor/>");
        }
        out.push_str("</m:rPr>");
    }
    out.push_str("<m:t>");
    out.push_str(&escape_text(text));
    out.push_str("</m:t></m:r>");
}

/// A formula being read, group by group.
pub(super) struct Formula {
    groups: Vec<Group>,
}

impl Formula {
    /// A formula whose `\mmath` group has started.
    pub(super) fn new() -> Formula {
        Formula {
            groups: vec![Group::default()],
        }
    }

    pub(super) fn open(&mut self) {
        self.groups.push(Group::default());
    }

    pub(super) fn word(&mut self, word: &str, param: Option<i32>) {
        let Some(group) = self.groups.last_mut() else { return };
        let fresh = group.element.is_none() && group.xml.is_empty() && group.text.is_empty();
        if fresh
            && matches!(
                word,
                "mmathPict" | "pict" | "shppict" | "nonshppict" | "picprop" | "shp"
            )
        {
            group.skipped = true;
        }
        if group.skipped {
            return;
        }
        if fresh && let Some(element) = word.strip_prefix('m').and_then(element_named) {
            group.element = Some(element);
            return;
        }
        match word {
            "msty" => group.upright = param == Some(0),
            "mnor" => group.normal = true,
            _ => {}
        }
    }

    pub(super) fn char(&mut self, c: char) {
        if let Some(group) = self.groups.last_mut().filter(|group| !group.skipped) {
            group.text.push(c);
        }
    }

    pub(super) fn close(&mut self) {
        if self.groups.len() < 2 {
            return;
        }
        let Some(mut group) = self.groups.pop() else { return };
        let Some(outer) = self.groups.last_mut() else { return };
        if group.skipped {
            return;
        }
        match group.element {
            None => {
                if !group.xml.is_empty() {
                    outer.flush();
                    outer.xml.push_str(&group.xml);
                }
                outer.text.push_str(&group.text);
            }
            Some("r") => {
                outer.flush();
                write_run(&group.text, group.upright, group.normal, &mut outer.xml);
            }
            Some(element) if VALUED.contains(&element) => {
                outer.flush();
                outer.xml.push_str(&format!("<m:{element} m:val=\""));
                outer.xml.push_str(&escape_text(&group.text));
                outer.xml.push_str("\"/>");
            }
            Some(element) => {
                group.flush();
                outer.flush();
                outer.xml.push_str(&format!("<m:{element}>"));
                outer.xml.push_str(&group.xml);
                outer.xml.push_str(&format!("</m:{element}>"));
            }
        }
    }

    /// The formula as LaTeX, and whether it stands on lines of its own.
    pub(super) fn finish(mut self) -> Option<(String, bool)> {
        while self.groups.len() > 1 {
            self.close();
        }
        let mut root = self.groups.pop()?;
        root.flush();
        crate::docx::math::from_omml(&root.xml)
    }
}

fn element_named(name: &str) -> Option<&'static str> {
    ELEMENTS.iter().copied().find(|element| *element == name)
}
