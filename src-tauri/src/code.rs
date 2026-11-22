//! Syntax highlighting via syntect + two-face (FR-15).
//!
//! Classed HTML output (no inline styles) so the frontend can switch themes
//! with CSS classes; line numbers are CSS `::before` counters so Copy always
//! yields clean text.

use std::path::Path;
use std::sync::OnceLock;

use syntect::highlighting::Theme;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::detection;

pub struct CodeHtml {
    pub html: String,
    pub lang: String,
}

struct Themes {
    syntax_set: SyntaxSet,
    theme_light: Theme,
    theme_dark: Theme,
}

static THEMES: OnceLock<Themes> = OnceLock::new();

fn themes() -> &'static Themes {
    THEMES.get_or_init(|| {
        let theme_set = two_face::theme::extra();
        Themes {
            syntax_set: two_face::syntax::extra_newlines(),
            theme_light: theme_set
                .get(two_face::theme::EmbeddedThemeName::InspiredGithub)
                .clone(),
            theme_dark: theme_set
                .get(two_face::theme::EmbeddedThemeName::Base16OceanDark)
                .clone(),
        }
    })
}

/// Resolve the syntect syntax for a path (special filenames, then extension).
fn syntax_for(path: &Path) -> Option<&'static SyntaxReference> {
    let t = themes();
    let name = path.file_name()?.to_string_lossy().into_owned();
    let upper = name.to_ascii_uppercase();
    if upper == "DOCKERFILE" || upper == "CONTAINERFILE" {
        if let Some(s) = t.syntax_set.find_syntax_by_token("dockerfile") {
            return Some(s);
        }
    }
    if upper == "MAKEFILE" || upper == "GNUMAKEFILE" {
        if let Some(s) = t.syntax_set.find_syntax_by_token("makefile") {
            return Some(s);
        }
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    t.syntax_set
        .find_syntax_by_extension(&ext.to_ascii_lowercase())
        .or_else(|| t.syntax_set.find_syntax_by_first_line(&name))
}

/// Highlight text into classed HTML with per-line wrappers. The generator's
/// spans may cross newlines; `split_lines_balanced` closes and reopens them
/// per line so wrapping and CSS-counter line numbers always work.
pub fn highlight(path: &Path, code: &str) -> CodeHtml {
    let t = themes();
    let lang = detection::language_label(
        &path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "txt".into()),
    );
    let Some(syntax) = syntax_for(path) else {
        return CodeHtml { html: escape_and_split(code), lang };
    };
    let mut gen = ClassedHTMLGenerator::new_with_class_style(syntax, &t.syntax_set, ClassStyle::Spaced);
    for line in LinesWithEndings::from(code) {
        let _ = gen.parse_html_for_line_which_includes_newline(line);
    }
    CodeHtml { html: split_lines_balanced(&gen.finalize()), lang }
}

/// CSS for both embedded themes, re-scoped under `.theme-light` / `.theme-dark`.
pub fn theme_css() -> String {
    let t = themes();
    let light = css_for_theme_with_class_style(&t.theme_light, ClassStyle::Spaced).unwrap_or_default();
    let dark = css_for_theme_with_class_style(&t.theme_dark, ClassStyle::Spaced).unwrap_or_default();
    format!("{}\n{}\n", rescope(&light, "theme-light"), rescope(&dark, "theme-dark"))
}

fn rescope(css: &str, wrapper: &str) -> String {
    // Generated rules start at `.code`; re-scope under our wrapper class so
    // both themes can coexist and the frontend toggles one class.
    css.replace(".code .", &format!(".{wrapper} .code ."))
        .replace(".code {", &format!(".{wrapper} .code {{"))
        .replace(".code,", &format!(".{wrapper} .code,"))
}

/// Split generated HTML into per-line `<div class="cl">` wrappers, closing
/// spans at each newline and reopening them on the next line. The newline
/// characters inside spans become the div boundaries themselves.
fn split_lines_balanced(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 256);
    let mut open_classes: Vec<String> = Vec::new();
    let mut line = String::new();

    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' if chars.peek() == Some(&'s') => {
                // <span ...> — capture the class attribute
                let mut tag = String::from("<");
                for c2 in chars.by_ref() {
                    tag.push(c2);
                    if c2 == '>' {
                        break;
                    }
                }
                if let Some(cls) = span_class(&tag) {
                    open_classes.push(cls);
                }
                line.push_str(&tag);
            }
            '<' if chars.peek() == Some(&'/') => {
                let mut tag = String::from("<");
                for c2 in chars.by_ref() {
                    tag.push(c2);
                    if c2 == '>' {
                        break;
                    }
                }
                if tag == "</span>" {
                    open_classes.pop();
                }
                line.push_str(&tag);
            }
            '\n' => {
                for _ in &open_classes {
                    out.push_str("</span>");
                }
                out.push_str("<div class=\"cl\">");
                out.push_str(&line);
                out.push_str("</div>");
                for cls in &open_classes {
                    out.push_str(&format!("<span class=\"{cls}\">"));
                }
                line.clear();
            }
            _ => line.push(c),
        }
    }
    // Flush the final line only if it carries visible content — trailing
    // span closes from the generator produce an empty last fragment.
    let has_content = line
        .replace("</span>", "")
        .chars()
        .any(|c| !c.is_whitespace());
    if has_content {
        for _ in &open_classes {
            out.push_str("</span>");
        }
        out.push_str("<div class=\"cl\">");
        out.push_str(&line);
        out.push_str("</div>");
    }
    out
}

fn span_class(tag: &str) -> Option<String> {
    let idx = tag.find("class=\"")? + 7;
    let end = tag[idx..].find('"')? + idx;
    Some(tag[idx..end].to_string())
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_and_split(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 128);
    for line in LinesWithEndings::from(code) {
        out.push_str("<div class=\"cl\">");
        out.push_str(&escape_html(line.trim_end_matches(['\r', '\n'])));
        out.push_str("</div>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn highlights_rust() {
        let p = PathBuf::from("main.rs");
        let out = highlight(&p, "fn main() {\n    println!(\"hi\");\n}\n");
        assert!(out.html.contains("cl"));
        assert!(out.html.contains("fn") || out.html.contains("main"));
        assert!(out.lang == "RS");
    }

    #[test]
    fn highlights_python_and_special_names() {
        let out = highlight(&PathBuf::from("script.py"), "def x():\n    pass\n");
        assert!(!out.html.is_empty());
        let out = highlight(&PathBuf::from("Dockerfile"), "FROM alpine\n");
        assert!(!out.html.is_empty());
    }

    #[test]
    fn unknown_extension_falls_back_to_escaped_text() {
        let out = highlight(&PathBuf::from("weird.zzz"), "a < b & c\n");
        assert!(out.html.contains("&lt;"));
    }

    #[test]
    fn theme_css_scopes_both_themes() {
        let css = theme_css();
        assert!(css.contains(".theme-light .code"));
        assert!(css.contains(".theme-dark .code"));
    }

    #[test]
    fn line_wrapping_excludes_numbers_from_text() {
        let out = highlight(&PathBuf::from("a.py"), "x = 1\ny = 2\n");
        assert_eq!(out.html.matches("<div class=\"cl\">").count(), 2);
    }
}

#[cfg(test)]
mod debug_tests {
    #[test]
    fn debug_html() {
        let out = super::highlight(&std::path::PathBuf::from("a.py"), "x = 1\ny = 2\n");
        println!("HTML: {:?}", out.html);
    }
}

#[cfg(test)]
mod dbg2 {
    #[test]
    fn dbg_lines() {
        let out = super::highlight(&std::path::PathBuf::from("a.py"), "x = 1\ny = 2\n");
        println!("HTML2: {:?}", out.html);
    }
}
