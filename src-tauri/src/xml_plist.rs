//! XML tree (Tree/Raw) and plist (XML + binary) key/type/value listings (FR-29).

use serde::Serialize;

#[derive(Serialize)]
pub struct XmlPreview {
    pub tree_html: String,
    pub raw_source: String,
    pub error: Option<String>,
}

/// Render a nested tree from the XML events: elements as nodes with
/// attributes shown inline; text content kept as-is (escaped).
pub fn xml_tree(content: &str) -> XmlPreview {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);

    let mut out = String::with_capacity(content.len() * 2);
    let mut depth = 0usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                let attrs = render_attrs(&e);
                out.push_str(&format!(
                    "<div class=\"xml-el\" style=\"padding-left:{}ch\"><span class=\"xml-tag\">&lt;{name}{attrs}&gt;</span></div>",
                    depth * 2
                ));
                depth += 1;
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_string();
                let attrs = render_attrs(&e);
                out.push_str(&format!(
                    "<div class=\"xml-el\" style=\"padding-left:{}ch\"><span class=\"xml-tag\">&lt;{name}{attrs} /&gt;</span></div>",
                    depth * 2
                ));
            }
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Text(t)) => {
                let text = t.xml_content(quick_xml::XmlVersion::Implicit1_0);
                if !text.trim().is_empty() {
                    out.push_str(&format!(
                        "<div class=\"xml-text\" style=\"padding-left:{}ch\">{}</div>",
                        depth * 2,
                        crate::code::escape_html(text.trim())
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return XmlPreview {
                    tree_html: String::new(),
                    raw_source: content.to_string(),
                    error: Some(e.to_string()),
                };
            }
            _ => {}
        }
        buf.clear();
    }
    XmlPreview { tree_html: out, raw_source: content.to_string(), error: None }
}

fn render_attrs(e: &quick_xml::events::BytesStart) -> String {
    let mut s = String::new();
    for a in e.attributes().flatten() {
        let k = a.key.as_ref().to_string();
        let v = a.value.as_ref().to_string();
        s.push_str(&format!(
            " <span class=\"xml-attr\">{}=\"{}\"</span>",
            crate::code::escape_html(&k),
            crate::code::escape_html(&v)
        ));
    }
    s
}

#[derive(Serialize)]
pub struct PlistPreview {
    pub entries: Vec<PlistEntry>,
    pub raw_xml: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct PlistEntry {
    pub key: String,
    pub kind: String,
    pub value: String,
    pub depth: usize,
}

/// Parse XML or binary plists (plist crate is pure Rust, works on Windows).
pub fn plist_entries(bytes: &[u8]) -> PlistPreview {
    match plist::Value::from_reader(std::io::Cursor::new(bytes)) {
        Ok(v) => {
            let mut entries = Vec::new();
            flatten(&v, 0, &mut entries);
            let raw_xml = if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<") {
                Some(String::from_utf8_lossy(bytes).into_owned())
            } else {
                None
            };
            PlistPreview { entries, raw_xml, error: None }
        }
        Err(e) => PlistPreview { entries: Vec::new(), raw_xml: None, error: Some(e.to_string()) },
    }
}

fn flatten(v: &plist::Value, depth: usize, out: &mut Vec<PlistEntry>) {
    match v {
        plist::Value::Dictionary(d) => {
            for (k, val) in d.iter() {
                let kind = type_name(val);
                if is_container(val) {
                    out.push(PlistEntry {
                        key: k.clone(),
                        kind: kind.into(),
                        value: summary(val),
                        depth,
                    });
                    flatten(val, depth + 1, out);
                } else {
                    out.push(PlistEntry {
                        key: k.clone(),
                        kind: kind.into(),
                        value: scalar_string(val),
                        depth,
                    });
                }
            }
        }
        plist::Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                let kind = type_name(val);
                if is_container(val) {
                    out.push(PlistEntry {
                        key: format!("[{i}]"),
                        kind: kind.into(),
                        value: summary(val),
                        depth,
                    });
                    flatten(val, depth + 1, out);
                } else {
                    out.push(PlistEntry {
                        key: format!("[{i}]"),
                        kind: kind.into(),
                        value: scalar_string(val),
                        depth,
                    });
                }
            }
        }
        _ => {}
    }
}

fn is_container(v: &plist::Value) -> bool {
    matches!(v, plist::Value::Dictionary(_) | plist::Value::Array(_))
}

fn type_name(v: &plist::Value) -> &'static str {
    match v {
        plist::Value::Dictionary(_) => "dict",
        plist::Value::Array(_) => "array",
        plist::Value::String(_) => "string",
        plist::Value::Integer(_) => "integer",
        plist::Value::Real(_) => "real",
        plist::Value::Boolean(_) => "bool",
        plist::Value::Date(_) => "date",
        plist::Value::Data(_) => "data",
        _ => "other",
    }
}

fn summary(v: &plist::Value) -> String {
    match v {
        plist::Value::Dictionary(d) => format!("{{ {} keys }}", d.len()),
        plist::Value::Array(a) => format!("[ {} items ]", a.len()),
        _ => scalar_string(v),
    }
}

fn scalar_string(v: &plist::Value) -> String {
    match v {
        plist::Value::String(s) => crate::code::escape_html(s),
        plist::Value::Integer(i) => i.to_string(),
        plist::Value::Real(r) => r.to_string(),
        plist::Value::Boolean(b) => b.to_string(),
        plist::Value::Date(d) => format!("{:?}", d),
        plist::Value::Data(d) => format!("<data {} bytes>", d.len()),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_tree_and_attrs() {
        let xml = r#"<?xml version="1.0"?>
<root version="2">
  <item id="1">alpha</item>
  <empty/>
</root>"#;
        let p = xml_tree(xml);
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(p.tree_html.contains("xml-tag"));
        assert!(p.tree_html.contains("root"));
        assert!(p.tree_html.contains("version=\"2\""));
        assert!(p.tree_html.contains("alpha"));
    }

    #[test]
    fn xml_parse_error_reported() {
        let p = xml_tree("<a><b></a>");
        assert!(p.error.is_some());
        assert_eq!(p.raw_source, "<a><b></a>");
    }

    #[test]
    fn plist_xml() {
        let bytes = b"<?xml version=\"1.0\"?><plist><dict><key>Name</key><string>App</string><key>Count</key><integer>3</integer></dict></plist>";
        let p = plist_entries(bytes);
        assert!(p.error.is_none());
        assert!(p.entries.iter().any(|e| e.key == "Name" && e.value == "App"));
        assert!(p.entries.iter().any(|e| e.key == "Count" && e.kind == "integer"));
        assert!(p.raw_xml.is_some());
    }

    #[test]
    fn plist_binary() {
        // Hand-built binary plist via the plist crate itself.
        let mut dict = plist::Dictionary::new();
        dict.insert("Key".into(), plist::Value::String("Value".into()));
        dict.insert("Num".into(), plist::Value::Integer(plist::Integer::from(42)));
        let mut buf = Vec::new();
        plist::Value::Dictionary(dict).to_writer_binary(&mut buf).unwrap();
        assert!(!buf.starts_with(b"<"), "must be binary");
        let p = plist_entries(&buf);
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(p.entries.iter().any(|e| e.key == "Key" && e.value == "Value"));
        assert!(p.raw_xml.is_none());
    }
}
