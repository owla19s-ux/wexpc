//! Pure, conservative path-scope overlap preflight for parallel Agent work.
//! This is NOT an authorization engine, lock manager, canonical filesystem resolver,
//! or a proof that two operations cannot conflict.
//! Rejects ambiguous paths rather than allowing an unsafe parallel write.
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScopeError {
    Empty,
    Relative,
    Traversal,
    Unsupported,
}
impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopePath {
    volume: String,
    segments: Vec<String>,
}

impl ScopePath {
    /// Lexical Windows drive-absolute paths only (e.g. C:\\repo\\src).
    /// Reject UNC, device prefixes, relative paths and dot/dot-dot segments.
    /// Case folding is ASCII-only: a conservative prototype, not Windows
    /// Unicode/canonical path equivalence. Do not use this to grant access.
    pub fn parse(raw: &str) -> Result<Self, ScopeError> {
        if raw.is_empty() { return Err(ScopeError::Empty); }
        let path = raw.replace('/', "\\");
        let bytes = path.as_bytes();
        if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
            return Err(ScopeError::Relative);
        }
        if (bytes[0] == 92 && bytes.get(1) == Some(&92)) || bytes[2..].contains(&58) {
            return Err(ScopeError::Unsupported);
        }
        let mut segments = Vec::new();
        for seg in path[3..].split('\\') {
            if seg.is_empty() { continue; }
            if seg == "." || seg == ".." { return Err(ScopeError::Traversal); }
            // Windows device names and non-ASCII case folding require native path resolution.
            if !seg.is_ascii() { return Err(ScopeError::Unsupported); }
            let stem = seg.split('.').next().unwrap_or("");
            let device = stem.to_ascii_uppercase();
            if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL" |
                "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" |
                "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9") {
                return Err(ScopeError::Unsupported);
            }
            if seg.ends_with('.') || seg.ends_with(' ') || seg.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) {
                return Err(ScopeError::Unsupported);
            }
            segments.push(seg.to_ascii_lowercase());
        }
        Ok(Self { volume: path[..1].to_ascii_lowercase(), segments })
    }

    /// True if same drive and one path is an ancestor of the other, including equal paths.
    pub fn overlaps(&self, other: &Self) -> bool {
        if self.volume != other.volume { return false; }
        let common = self.segments.len().min(other.segments.len());
        self.segments[..common] == other.segments[..common]
    }
}

pub fn may_overlap(left: &[String], right: &[String]) -> Result<bool, ScopeError> {
    let left: Vec<_> = left.iter().map(|s| ScopePath::parse(s)).collect::<Result<_, _>>()?;
    let right: Vec<_> = right.iter().map(|s| ScopePath::parse(s)).collect::<Result<_, _>>()?;
    Ok(left.iter().any(|a| right.iter().any(|b| a.overlaps(b))))
}

#[cfg(test)]
mod tests;
