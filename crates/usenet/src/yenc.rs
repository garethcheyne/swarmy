//! yEnc, the encoding nearly every Usenet binary uses.
//!
//! An article body looks like:
//!
//! ```text
//! =ybegin part=1 total=10 line=128 size=7340032 name=file.bin
//! =ypart begin=1 end=768000
//! ...encoded lines...
//! =yend size=768000 part=1 pcrc32=1a2b3c4d
//! ```
//!
//! Each byte is stored as `(byte + 42) % 256`; NUL, LF, CR and `=` (and a
//! few others at line edges) are escaped as `=` followed by `(byte + 64)`.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// File name from the header; untrusted, sanitise before using it.
    pub name: String,
    /// Size of the whole file.
    pub file_size: u64,
    pub part: u32,
    pub total: u32,
    /// Where this part goes in the file: 0-based offset.
    pub offset: u64,
    pub data: Vec<u8>,
    /// False if the part's CRC32 or length didn't match: the bytes are kept
    /// (PAR2 can repair them) but the file needs checking.
    pub intact: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum YencError {
    #[error("not a yEnc article (no =ybegin line)")]
    NotYenc,
    #[error("the yEnc header is malformed: {0}")]
    BadHeader(&'static str),
}

/// Find `key=` in a header line and parse what follows, up to a space.
fn field<T: std::str::FromStr>(line: &str, key: &str) -> Option<T> {
    let mut search = 0;
    while let Some(i) = line[search..].find(key) {
        let at = search + i;
        // Match whole keys only: "size=" must not match inside "psize=".
        let ok = at == 0 || line.as_bytes()[at - 1] == b' ';
        let start = at + key.len();
        if ok {
            let end = line[start..].find(' ').map_or(line.len(), |e| start + e);
            return line[start..end].trim().parse().ok();
        }
        search = start;
    }
    None
}

/// Decode one article body (already dot-unstuffed, lines ending in CRLF or LF).
pub fn decode(body: &[u8]) -> Result<Part, YencError> {
    let mut lines = body.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l));
    // Skip anything before =ybegin (some posters add text).
    let header = loop {
        match lines.next() {
            Some(l) if l.starts_with(b"=ybegin ") => break String::from_utf8_lossy(l).into_owned(),
            Some(_) => continue,
            None => return Err(YencError::NotYenc),
        }
    };
    let file_size: u64 = field(&header, "size=").ok_or(YencError::BadHeader("no size"))?;
    let part: u32 = field(&header, "part=").unwrap_or(1);
    let total: u32 = field(&header, "total=").unwrap_or(1);
    // The name runs to the end of the line and may contain spaces.
    let name = header.find(" name=").map(|i| header[i + 6..].trim().to_string()).unwrap_or_default();

    let mut begin: u64 = 1;
    let mut end: Option<u64> = None;
    let mut data = Vec::with_capacity(800 * 1024);
    let mut trailer: Option<String> = None;
    let mut first = true;
    for line in lines {
        if first && line.starts_with(b"=ypart ") {
            let l = String::from_utf8_lossy(line);
            begin = field(&l, "begin=").ok_or(YencError::BadHeader("ypart without begin"))?;
            end = field(&l, "end=");
            first = false;
            continue;
        }
        first = false;
        if line.starts_with(b"=yend") {
            trailer = Some(String::from_utf8_lossy(line).into_owned());
            break;
        }
        let mut escaped = false;
        for &b in line {
            if escaped {
                data.push(b.wrapping_sub(64).wrapping_sub(42));
                escaped = false;
            } else if b == b'=' {
                escaped = true;
            } else {
                data.push(b.wrapping_sub(42));
            }
        }
    }
    if begin == 0 {
        return Err(YencError::BadHeader("begin=0"));
    }
    let mut intact = trailer.is_some();
    if let Some(t) = &trailer {
        if let Some(size) = field::<u64>(t, "size=")
            && size != data.len() as u64
        {
            intact = false;
        }
        // Multi-part articles carry pcrc32; single-part ones crc32.
        let want = field_hex(t, "pcrc32=").or_else(|| if total <= 1 { field_hex(t, "crc32=") } else { None });
        if let Some(want) = want
            && want != crc32fast::hash(&data)
        {
            intact = false;
        }
    }
    if let Some(end) = end
        && end + 1 - begin != data.len() as u64
    {
        intact = false;
    }
    Ok(Part { name, file_size, part, total, offset: begin - 1, data, intact })
}

fn field_hex(line: &str, key: &str) -> Option<u32> {
    let s: String = field(line, key)?;
    u32::from_str_radix(s.trim(), 16).ok()
}

/// Encode one part (for tests and the test poster). Returns the article body
/// with CRLF line endings, not yet dot-stuffed.
pub fn encode(file: &[u8], name: &str, part: u32, total: u32, begin: usize, end: usize, line_len: usize) -> Vec<u8> {
    let chunk = &file[begin..end];
    let mut out = Vec::with_capacity(chunk.len() * 102 / 100 + 256);
    if total > 1 {
        out.extend_from_slice(format!("=ybegin part={part} total={total} line={line_len} size={} name={name}\r\n", file.len()).as_bytes());
        out.extend_from_slice(format!("=ypart begin={} end={end}\r\n", begin + 1).as_bytes());
    } else {
        out.extend_from_slice(format!("=ybegin line={line_len} size={} name={name}\r\n", file.len()).as_bytes());
    }
    let mut col = 0;
    for (i, &b) in chunk.iter().enumerate() {
        let e = b.wrapping_add(42);
        let edge = col == 0 || i + 1 == chunk.len() || col + 1 >= line_len;
        let critical = matches!(e, 0 | b'\n' | b'\r' | b'=') || (edge && matches!(e, b'\t' | b' ')) || (col == 0 && e == b'.');
        if critical {
            out.push(b'=');
            out.push(e.wrapping_add(64));
            col += 2;
        } else {
            out.push(e);
            col += 1;
        }
        if col >= line_len {
            out.extend_from_slice(b"\r\n");
            col = 0;
        }
    }
    if col > 0 {
        out.extend_from_slice(b"\r\n");
    }
    let crc = crc32fast::hash(chunk);
    if total > 1 {
        out.extend_from_slice(format!("=yend size={} part={part} pcrc32={crc:08x}\r\n", chunk.len()).as_bytes());
    } else {
        out.extend_from_slice(format!("=yend size={} crc32={crc:08x}\r\n", chunk.len()).as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + i / 3) as u8).collect()
    }

    #[test]
    fn round_trips_every_byte_value() {
        let file: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let body = encode(&file, "all bytes.bin", 1, 1, 0, file.len(), 128);
        let p = decode(&body).unwrap();
        assert_eq!(p.data, file);
        assert!(p.intact);
        assert_eq!(p.name, "all bytes.bin");
        assert_eq!(p.offset, 0);
        assert_eq!(p.file_size, 5000);
    }

    #[test]
    fn multipart_offsets_and_crc() {
        let file = sample(10_000);
        let body = encode(&file, "x.bin", 2, 3, 4000, 8000, 128);
        let p = decode(&body).unwrap();
        assert_eq!(p.part, 2);
        assert_eq!(p.total, 3);
        assert_eq!(p.offset, 4000);
        assert_eq!(p.data, &file[4000..8000]);
        assert!(p.intact);
    }

    #[test]
    fn detects_corruption() {
        let file = sample(3000);
        let mut body = encode(&file, "x.bin", 1, 2, 0, 1500, 128);
        // Flip a data byte on the third line.
        let i = body.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(3).unwrap().0 + 5;
        body[i] = body[i].wrapping_add(1);
        let p = decode(&body).unwrap();
        assert!(!p.intact);
    }

    #[test]
    fn rejects_non_yenc() {
        assert_eq!(decode(b"hello\r\nworld\r\n"), Err(YencError::NotYenc));
    }

    #[test]
    fn header_fields_match_whole_keys() {
        assert_eq!(field::<u64>("=yend size=10 part=1 pcrc32=ab", "size="), Some(10));
        assert_eq!(field::<u64>("=ybegin psize=99 size=10", "size="), Some(10));
    }
}
