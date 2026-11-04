//! NZB files: which articles make up a download.
//!
//! An NZB lists files, each split into segments (one Usenet article each)
//! identified by message-id. We keep what downloading needs and tolerate the
//! oddities real indexers produce (entities in subjects, missing metadata,
//! duplicate segment numbers).

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Nzb {
    /// `<meta type="...">` values, e.g. title, password, category.
    pub meta: BTreeMap<String, String>,
    pub files: Vec<NzbFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NzbFile {
    pub subject: String,
    pub poster: String,
    pub date: i64,
    pub groups: Vec<String>,
    /// Sorted by number, one per part.
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub number: u32,
    /// Encoded article size, as the NZB states it.
    pub bytes: u64,
    /// Without the angle brackets.
    pub message_id: String,
}

impl Nzb {
    pub fn parse(xml: &[u8]) -> anyhow::Result<Self> {
        let mut reader = Reader::from_reader(xml);
        reader.config_mut().trim_text(true);
        let mut nzb = Nzb::default();
        let mut buf = Vec::new();
        let mut file: Option<NzbFile> = None;
        let mut segment: Option<Segment> = None;
        let mut in_group = false;
        let mut meta_key: Option<String> = None;
        let mut text = String::new();
        loop {
            match reader.read_event_into(&mut buf).context("reading the NZB")? {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"file" => {
                    let mut f = NzbFile::default();
                    for a in e.attributes().flatten() {
                        let v = attr(&a);
                        match a.key.local_name().as_ref() {
                            b"subject" => f.subject = v,
                            b"poster" => f.poster = v,
                            b"date" => f.date = v.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                    file = Some(f);
                }
                Event::Start(e) if e.local_name().as_ref() == b"segment" => {
                    let mut s = Segment::default();
                    for a in e.attributes().flatten() {
                        let v = attr(&a);
                        match a.key.local_name().as_ref() {
                            b"number" => s.number = v.trim().parse().unwrap_or(0),
                            b"bytes" => s.bytes = v.trim().parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                    segment = Some(s);
                    text.clear();
                }
                Event::Start(e) if e.local_name().as_ref() == b"group" => {
                    in_group = true;
                    text.clear();
                }
                Event::Start(e) if e.local_name().as_ref() == b"meta" => {
                    meta_key = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == b"type")
                        .map(|a| attr(&a).to_lowercase());
                    text.clear();
                }
                Event::Text(t) => text.push_str(&t.decode().unwrap_or_default()),
                // Entities arrive as their own events: &lt; &#62; and so on.
                Event::GeneralRef(r) => {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        text.push(c);
                    } else if let Ok(name) = r.decode() {
                        match quick_xml::escape::resolve_predefined_entity(&name) {
                            Some(v) => text.push_str(v),
                            None => {
                                text.push('&');
                                text.push_str(&name);
                                text.push(';');
                            }
                        }
                    }
                }
                Event::CData(t) => text.push_str(&String::from_utf8_lossy(&t)),
                Event::End(e) => match e.local_name().as_ref() {
                    b"segment" => {
                        if let (Some(mut s), Some(f)) = (segment.take(), file.as_mut()) {
                            s.message_id = clean_message_id(&text);
                            if !s.message_id.is_empty() {
                                f.segments.push(s);
                            }
                        }
                    }
                    b"group" if in_group => {
                        in_group = false;
                        if let Some(f) = file.as_mut() {
                            let g = text.trim().to_string();
                            if !g.is_empty() {
                                f.groups.push(g);
                            }
                        }
                    }
                    b"meta" => {
                        if let Some(k) = meta_key.take() {
                            nzb.meta.insert(k, text.trim().to_string());
                        }
                    }
                    b"file" => {
                        if let Some(mut f) = file.take() {
                            f.segments.sort_by_key(|s| s.number);
                            f.segments.dedup_by_key(|s| s.number);
                            if !f.segments.is_empty() {
                                nzb.files.push(f);
                            }
                        }
                    }
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        if nzb.files.is_empty() {
            bail!("the NZB lists no files");
        }
        Ok(nzb)
    }

    /// Total encoded size of every segment.
    pub fn bytes(&self) -> u64 {
        self.files.iter().flat_map(|f| &f.segments).map(|s| s.bytes).sum()
    }
}

impl NzbFile {
    /// The file name, from the subject's quoted name (the usual convention:
    /// `[01/10] - "name.part01.rar" yEnc (1/50)`), else the whole subject.
    /// The yEnc header's name, once downloaded, is more reliable.
    pub fn name_from_subject(&self) -> String {
        let s = &self.subject;
        if let (Some(a), Some(b)) = (s.find('"'), s.rfind('"'))
            && b > a + 1
        {
            return sanitise(&s[a + 1..b]);
        }
        sanitise(s)
    }

    pub fn bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.bytes).sum()
    }

    /// PAR2 files are fetched last (and only the index at first), since
    /// they're only needed if something is damaged.
    pub fn is_par2(&self) -> bool {
        self.name_from_subject().to_ascii_lowercase().ends_with(".par2")
    }
}

fn attr(a: &quick_xml::events::attributes::Attribute) -> String {
    a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).unwrap_or_default()
}

fn clean_message_id(s: &str) -> String {
    s.trim().trim_start_matches('<').trim_end_matches('>').trim().to_string()
}

/// A safe single path component: no separators, no `..`, no control chars.
pub fn sanitise(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_matches('.').trim().to_string();
    if out.is_empty() {
        out = "file".into();
    }
    if out.len() > 200 {
        let mut cut = 200;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.1//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd">
<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">
  <head>
    <meta type="title">Big Buck Bunny</meta>
    <meta type="password">secret</meta>
  </head>
  <file poster="someone &lt;a@b.c&gt;" date="1700000000" subject="[1/2] - &quot;bbb.part1.rar&quot; yEnc (1/2)">
    <groups><group>alt.binaries.test</group></groups>
    <segments>
      <segment bytes="400000" number="2">part2of2.abc@example</segment>
      <segment bytes="400000" number="1">&lt;part1of2.abc@example&gt;</segment>
    </segments>
  </file>
  <file poster="someone" date="1700000000" subject="[2/2] - &quot;bbb.par2&quot; yEnc (1/1)">
    <groups><group>alt.binaries.test</group></groups>
    <segments><segment bytes="1000" number="1">par.abc@example</segment></segments>
  </file>
</nzb>"#;

    #[test]
    fn parses_files_segments_and_meta() {
        let nzb = Nzb::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(nzb.meta.get("title").map(String::as_str), Some("Big Buck Bunny"));
        assert_eq!(nzb.meta.get("password").map(String::as_str), Some("secret"));
        assert_eq!(nzb.files.len(), 2);
        let f = &nzb.files[0];
        assert_eq!(f.poster, "someone <a@b.c>");
        assert_eq!(f.groups, vec!["alt.binaries.test"]);
        assert_eq!(f.segments[0].number, 1);
        assert_eq!(f.segments[0].message_id, "part1of2.abc@example");
        assert_eq!(f.name_from_subject(), "bbb.part1.rar");
        assert!(nzb.files[1].is_par2());
        assert_eq!(nzb.bytes(), 801_000);
    }

    #[test]
    fn rejects_empty() {
        assert!(Nzb::parse(b"<nzb></nzb>").is_err());
    }

    #[test]
    fn sanitises_names() {
        assert_eq!(sanitise("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitise("a/b\\c"), "a_b_c");
        assert_eq!(sanitise("  ..  "), "file");
    }
}
