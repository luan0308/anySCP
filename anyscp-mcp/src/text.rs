//! Text encodings for remote files.
//!
//! Plenty of older Brazilian sites are still saved as Windows-1252 (often
//! called Latin-1). Reading them as UTF-8 mangles every "ç" and "ã", and
//! writing them back as UTF-8 breaks the page. So files are decoded with the
//! encoding they actually use and written back in that same encoding.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Windows1252,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf8Bom => "utf-8 with BOM",
            Encoding::Windows1252 => "windows-1252 (latin-1)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().replace('_', "-").as_str() {
            "utf-8" | "utf8" => Some(Encoding::Utf8),
            "utf-8-bom" | "utf8-bom" | "utf-8 with bom" => Some(Encoding::Utf8Bom),
            "latin-1" | "latin1" | "iso-8859-1" | "windows-1252" | "cp1252" | "ansi" => {
                Some(Encoding::Windows1252)
            }
            _ => None,
        }
    }
}

/// Bytes 0x80–0x9F in Windows-1252; the rest of the range maps 1:1 to Unicode.
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|&b| b == 0)
}

/// Decode with the encoding the file appears to use.
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(s) = std::str::from_utf8(rest) {
            return (s.to_string(), Encoding::Utf8Bom);
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), Encoding::Utf8),
        Err(e) => {
            // A multi-byte character cut off by a size limit is still UTF-8.
            if e.error_len().is_none() && e.valid_up_to() + 4 > bytes.len() {
                let s = std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default();
                return (s.to_string(), Encoding::Utf8);
            }
            (decode_cp1252(bytes), Encoding::Windows1252)
        }
    }
}

fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
            _ => b as char,
        })
        .collect()
}

pub fn encode(text: &str, enc: Encoding) -> Result<Vec<u8>> {
    match enc {
        Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            Ok(v)
        }
        Encoding::Windows1252 => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                let code = c as u32;
                if code < 0x80 || (0xA0..=0xFF).contains(&code) {
                    out.push(code as u8);
                } else if let Some(i) = CP1252_HIGH.iter().position(|&h| h == c) {
                    out.push(0x80 + i as u8);
                } else {
                    bail!(
                        "the character '{c}' cannot be stored in a windows-1252 file. Use an HTML \
                         entity instead (for example &#{code};) or convert the file to utf-8."
                    );
                }
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cp1252() {
        let original = "Promoção – “ótimo” € çã";
        let bytes = encode(original, Encoding::Windows1252).unwrap();
        let (back, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Windows1252);
        assert_eq!(back, original);
    }

    #[test]
    fn utf8_detected() {
        let (s, enc) = decode("ação".as_bytes());
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(s, "ação");
    }

    #[test]
    fn bom_preserved() {
        let bytes = encode("oi", Encoding::Utf8Bom).unwrap();
        let (s, enc) = decode(&bytes);
        assert_eq!((s.as_str(), enc), ("oi", Encoding::Utf8Bom));
    }

    #[test]
    fn truncated_utf8_is_still_utf8() {
        let full = "aãb".as_bytes();
        let (_, enc) = decode(&full[..2]);
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn emoji_rejected_in_cp1252() {
        assert!(encode("😀", Encoding::Windows1252).is_err());
    }
}
