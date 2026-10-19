//! Downloadable omnilingual CTC models and the digests clients verify them against.

/// A model release that unpacks to [`crate::MODEL_FILE`] and [`crate::TOKENS_FILE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: &'static str,
    pub parameters: &'static str,
    pub release: &'static str,
    pub archive_url: &'static str,
    pub model_bytes: u64,
    pub model_sha256: &'static str,
    pub tokens_sha256: &'static str,
}

pub const OMNILINGUAL_300M_INT8: ModelEntry = ModelEntry {
    id: "omnilingual-300m-ctc-int8",
    parameters: "300M",
    release: "2025-11-12",
    archive_url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/",
        "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8-2025-11-12.tar.bz2"
    ),
    model_bytes: 365_352_120,
    model_sha256: "e7c4e54ee4c4c47829cc6667d5d00ed8ea7bef1dcfeef0fce766f77752a2726c",
    tokens_sha256: "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31",
};

/// Higher accuracy than [`OMNILINGUAL_300M_INT8`], at roughly three times the size.
pub const OMNILINGUAL_1B_INT8: ModelEntry = ModelEntry {
    id: "omnilingual-1b-ctc-int8",
    parameters: "1B",
    release: "2025-11-12",
    archive_url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/",
        "sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12.tar.bz2"
    ),
    model_bytes: 1_031_628_252,
    model_sha256: "f7b74c964039162423b83e3fa950ce24810c9a635d9ff8468b5f4d142b7c1e8c",
    tokens_sha256: "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31",
};

pub const OMNILINGUAL_300M_V2_INT8: ModelEntry = ModelEntry {
    id: "omnilingual-300m-ctc-v2-int8",
    parameters: "300M",
    release: "2026-02-05",
    archive_url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/",
        "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05.tar.bz2"
    ),
    model_bytes: 365_841_453,
    model_sha256: "e3042b2f3b3ef0af2211bf99d2b4bf94a21f5ac0e9898827e7dd6d003a860e91",
    tokens_sha256: "7d99997ef207ff14c2cfe825f2aa037528ea250113cc3c6392bfe49326884ba6",
};

pub const OMNILINGUAL_1B_V2_INT8: ModelEntry = ModelEntry {
    id: "omnilingual-1b-ctc-v2-int8",
    parameters: "1B",
    release: "2026-02-05",
    archive_url: concat!(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/",
        "sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-v2-int8-2026-02-05.tar.bz2"
    ),
    model_bytes: 1_032_239_439,
    model_sha256: "8af72da192fc2c8567c328d4f8059bdf47f182a0369077893c295ef39740c637",
    tokens_sha256: "7d99997ef207ff14c2cfe825f2aa037528ea250113cc3c6392bfe49326884ba6",
};

pub const DEFAULT_MODEL: ModelEntry = OMNILINGUAL_300M_INT8;

pub const CATALOGUE: &[ModelEntry] = &[
    OMNILINGUAL_300M_INT8,
    OMNILINGUAL_1B_INT8,
    OMNILINGUAL_300M_V2_INT8,
    OMNILINGUAL_1B_V2_INT8,
];

pub fn find(id: &str) -> Option<&'static ModelEntry> {
    CATALOGUE.iter().find(|entry| entry.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_BASE: &str =
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/";

    fn is_sha256_hex(s: &str) -> bool {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
    }

    #[test]
    fn digests_are_well_formed() {
        assert!(CATALOGUE.iter().all(|e| is_sha256_hex(e.model_sha256)));
        assert!(CATALOGUE.iter().all(|e| is_sha256_hex(e.tokens_sha256)));
    }

    #[test]
    fn ids_are_unique() {
        assert!(CATALOGUE
            .iter()
            .all(|e| CATALOGUE.iter().filter(|o| o.id == e.id).count() == 1));
    }

    #[test]
    fn archives_come_from_the_release_base() {
        assert!(CATALOGUE
            .iter()
            .all(|e| e.archive_url.starts_with(RELEASE_BASE)));
    }

    #[test]
    fn find_resolves_the_default_by_id() {
        assert_eq!(find(DEFAULT_MODEL.id), Some(&DEFAULT_MODEL));
        assert_eq!(find("missing"), None);
    }

    #[test]
    fn each_1b_entry_is_larger_than_its_300m_release() {
        let larger = |small: &ModelEntry| {
            CATALOGUE
                .iter()
                .filter(|big| big.parameters == "1B" && big.release == small.release)
                .all(|big| small.model_bytes < big.model_bytes)
        };
        assert!(CATALOGUE
            .iter()
            .filter(|e| e.parameters == "300M")
            .all(larger));
    }

    #[test]
    fn the_default_is_a_300m_model() {
        assert_eq!(find(DEFAULT_MODEL.id).map(|e| e.parameters), Some("300M"));
    }
}
