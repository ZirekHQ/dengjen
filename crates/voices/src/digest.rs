use md5::Md5;
use sha2::{Digest as Hasher, Sha256};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Digest {
    Sha256(String),
    Md5(String),
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn hash_reader<D: Hasher>(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = D::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        match reader.read(&mut buffer)? {
            0 => return Ok(hex(&hasher.finalize())),
            n => hasher.update(buffer.get(..n).unwrap_or_default()),
        }
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

impl Digest {
    pub fn hex(&self) -> &str {
        let (Self::Sha256(hex) | Self::Md5(hex)) = self;
        hex
    }

    pub fn is_well_formed(&self) -> bool {
        let expected = match self {
            Self::Sha256(_) => 64,
            Self::Md5(_) => 32,
        };
        self.hex().len() == expected && self.hex().bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    pub fn matches(&self, path: &Path) -> io::Result<bool> {
        let file = File::open(path)?;
        let (expected, actual) = match self {
            Self::Sha256(expected) => (expected, hash_reader::<Sha256>(file)?),
            Self::Md5(expected) => (expected, hash_reader::<Md5>(file)?),
        };
        Ok(actual.eq_ignore_ascii_case(expected.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";

    fn file_with(content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn sha256_hex_matches_a_known_vector() {
        assert_eq!(sha256_hex(b"abc"), ABC_SHA256);
    }

    #[test]
    fn matches_accepts_the_right_digest_in_any_case() {
        let (_dir, path) = file_with(b"abc");
        assert!(Digest::Sha256(ABC_SHA256.into()).matches(&path).unwrap());
        assert!(Digest::Sha256(ABC_SHA256.to_uppercase())
            .matches(&path)
            .unwrap());
        assert!(Digest::Md5(ABC_MD5.into()).matches(&path).unwrap());
    }

    #[test]
    fn matches_rejects_other_content_and_wrong_algorithm() {
        let (_dir, path) = file_with(b"abd");
        assert!(!Digest::Sha256(ABC_SHA256.into()).matches(&path).unwrap());
        assert!(!Digest::Md5(ABC_MD5.into()).matches(&path).unwrap());
        let (_dir, abc) = file_with(b"abc");
        assert!(!Digest::Md5(ABC_SHA256.into()).matches(&abc).unwrap());
    }

    #[test]
    fn hex_and_well_formedness_follow_the_algorithm() {
        let sha = Digest::Sha256(ABC_SHA256.into());
        assert_eq!(sha.hex(), ABC_SHA256);
        assert!(sha.is_well_formed());
        assert!(Digest::Md5(ABC_MD5.into()).is_well_formed());
        assert!(!Digest::Md5(ABC_SHA256.into()).is_well_formed());
        assert!(!Digest::Sha256(ABC_MD5.into()).is_well_formed());
        assert!(!Digest::Md5("../../etc/passwd0000000000000000".into()).is_well_formed());
        assert!(!Digest::Md5(String::new()).is_well_formed());
    }

    #[test]
    fn matches_reports_a_missing_file() {
        let error = Digest::Md5(ABC_MD5.into()).matches(std::path::Path::new("/nonexistent/x"));
        assert_eq!(error.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }
}
