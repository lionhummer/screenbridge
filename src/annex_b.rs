use crate::error::{Error, Result};
use bytes::Bytes;
const MAX_ACCESS_UNIT: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct AccessUnit {
    pub data: Bytes,
    pub key: bool,
    pub codec: Option<String>,
}
#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
}
fn nals(b: &[u8]) -> Vec<(usize, usize, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < b.len() {
        let at = if b[i..i + 3] == [0, 0, 1] {
            Some(i + 3)
        } else if b[i..i + 4] == [0, 0, 0, 1] {
            Some(i + 4)
        } else {
            None
        };
        if let Some(at) = at.filter(|at| *at < b.len()) {
            out.push((i, at, b[at] & 31));
            i = at + 1;
        } else {
            i += 1;
        }
    }
    out
}
impl Parser {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<AccessUnit>> {
        if self.buffer.len().saturating_add(chunk.len()) > MAX_ACCESS_UNIT {
            self.buffer.clear();
            return Err(Error::Limit("H.264 access unit"));
        }
        self.buffer.extend_from_slice(chunk);
        let offsets: Vec<usize> = nals(&self.buffer)
            .into_iter()
            .filter(|n| n.2 == 9)
            .map(|n| n.0)
            .collect();
        let mut out = Vec::new();
        for w in offsets.windows(2) {
            out.push(Self::unit(&self.buffer[w[0]..w[1]]));
        }
        if offsets.len() > 1 {
            self.buffer.drain(..offsets[offsets.len() - 1]);
        }
        Ok(out)
    }
    fn unit(b: &[u8]) -> AccessUnit {
        let ns = nals(b);
        let codec = ns.iter().find(|n| n.2 == 7 && n.1 + 3 < b.len()).map(|n| {
            format!(
                "avc1.{:02x}{:02x}{:02x}",
                b[n.1 + 1],
                b[n.1 + 2],
                b[n.1 + 3]
            )
        });
        AccessUnit {
            data: Bytes::copy_from_slice(b),
            key: ns.iter().any(|n| n.2 == 5),
            codec,
        }
    }
    pub fn finish(&mut self) -> Option<AccessUnit> {
        let first = nals(&self.buffer).into_iter().find(|n| n.2 == 9)?;
        let has_slice = nals(&self.buffer).iter().any(|n| n.2 == 1 || n.2 == 5);
        let result = has_slice.then(|| Self::unit(&self.buffer[first.0..]));
        self.buffer.clear();
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &[u8] = &[0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 7, 66, 0, 31, 0, 0, 1, 5, 42];
    #[test]
    fn fragmented_keyframe_and_codec() {
        let mut p = Parser::default();
        let mut units = Vec::new();
        for b in KEY.iter().chain(KEY) {
            units.extend(p.push(&[*b]).unwrap())
        }
        assert_eq!(units.len(), 1);
        assert!(units[0].key);
        assert_eq!(units[0].codec.as_deref(), Some("avc1.42001f"));
        assert!(p.finish().is_some());
    }
    #[test]
    fn overflow_discards_buffer() {
        let mut p = Parser::default();
        assert!(p.push(&vec![0; MAX_ACCESS_UNIT + 1]).is_err());
        assert!(p.finish().is_none());
    }
    #[test]
    fn trailing_partial_is_not_emitted() {
        let mut p = Parser::default();
        assert!(p.push(&[0, 0, 1, 9, 0]).unwrap().is_empty());
        assert!(p.finish().is_none());
    }
}
