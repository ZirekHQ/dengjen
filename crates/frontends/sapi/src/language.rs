//! The `Language` attribute a voice registers with, as SAPI's hexadecimal LANGIDs.
//!
//! SAPI clients find voices by LANGID, and Windows has none for Kurmanji (`kmr`): its only Kurdish
//! LANGID, 0x0492, is Central Kurdish (Sorani, in Arabic script). [`KurdishPolicy`] chooses between
//! being accurate and being found by clients that only list languages Windows knows.
#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;

/// `LOCALE_CUSTOM_UNSPECIFIED`: what Windows reports for a language without a LANGID of its own.
pub const UNSPECIFIED: u16 = 0x1000;
/// `ckb-IQ`, Windows' only Kurdish LANGID.
pub const CENTRAL_KURDISH: u16 = 0x0492;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KurdishPolicy {
    /// Register Kurmanji voices as 0x1000 only. Accurate, but NVDA, which reads the first LANGID,
    /// reports the unknown locale `aa_ET`, and a client that looks voices up by a known language
    /// will not offer them.
    Unspecified,
    /// Register as Central Kurdish first and 0x1000 second. NVDA, which reads only the first
    /// LANGID, then reports `ckb` instead of a wrong locale, and clients asking for 0x1000 still
    /// find the voice. Kurmanji is labelled as Central Kurdish.
    #[default]
    CentralKurdishThenUnspecified,
    /// Register as Central Kurdish only, for clients that ignore voices with an unknown language.
    CentralKurdish,
}

impl FromStr for KurdishPolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unspecified" | "1000" => Ok(Self::Unspecified),
            "both" | "492;1000" => Ok(Self::CentralKurdishThenUnspecified),
            "ckb" | "492" => Ok(Self::CentralKurdish),
            other => Err(format!(
                "unknown Kurdish language policy `{other}`; use unspecified, both or ckb"
            )),
        }
    }
}

impl fmt::Display for KurdishPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unspecified => "unspecified",
            Self::CentralKurdishThenUnspecified => "both",
            Self::CentralKurdish => "ckb",
        })
    }
}

/// LANGIDs for the languages dengjen's Piper voices cover, by `language_REGION`.
const LANGIDS: &[(&str, u16)] = &[
    ("ar_JO", 0x2C01),
    ("ca_ES", 0x0403),
    ("cs_CZ", 0x0405),
    ("cy_GB", 0x0452),
    ("da_DK", 0x0406),
    ("de_DE", 0x0407),
    ("el_GR", 0x0408),
    ("en_GB", 0x0809),
    ("en_US", 0x0409),
    ("es_ES", 0x0C0A),
    ("es_MX", 0x080A),
    ("fa_IR", 0x0429),
    ("fi_FI", 0x040B),
    ("fr_FR", 0x040C),
    ("he_IL", 0x040D),
    ("hu_HU", 0x040E),
    ("is_IS", 0x040F),
    ("it_IT", 0x0410),
    ("ka_GE", 0x0437),
    ("kk_KZ", 0x043F),
    ("nl_NL", 0x0413),
    ("no_NO", 0x0414),
    ("pl_PL", 0x0415),
    ("pt_BR", 0x0416),
    ("pt_PT", 0x0816),
    ("ro_RO", 0x0418),
    ("ru_RU", 0x0419),
    ("sk_SK", 0x041B),
    ("sl_SI", 0x0424),
    ("sr_RS", 0x241A),
    ("sv_SE", 0x041D),
    ("sw_CD", 0x0441),
    ("tr_TR", 0x041F),
    ("uk_UA", 0x0422),
    ("vi_VN", 0x042A),
    ("zh_CN", 0x0804),
];

fn is_kurmanji(language: &str) -> bool {
    matches!(
        language
            .split(['_', '-'])
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ku" | "kmr")
    )
}

/// The LANGIDs for a voice language such as `ku_TR` or `en-GB`, most specific first.
pub fn langids(language: &str, kurdish: KurdishPolicy) -> Vec<u16> {
    if is_kurmanji(language) {
        return match kurdish {
            KurdishPolicy::Unspecified => vec![UNSPECIFIED],
            KurdishPolicy::CentralKurdishThenUnspecified => vec![CENTRAL_KURDISH, UNSPECIFIED],
            KurdishPolicy::CentralKurdish => vec![CENTRAL_KURDISH],
        };
    }
    let normalized = language.replace('-', "_");
    let exact = LANGIDS
        .iter()
        .find(|(tag, _)| tag.eq_ignore_ascii_case(&normalized))
        .map(|(_, langid)| *langid);
    let primary = normalized.split('_').next().and_then(|family| {
        LANGIDS
            .iter()
            .find(|(tag, _)| {
                tag.split('_')
                    .next()
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(family))
            })
            // The low ten bits of a LANGID are its primary language.
            .map(|(_, langid)| langid & 0x03FF)
    });
    match (exact, primary) {
        (Some(exact), Some(primary)) => vec![exact, primary],
        (None, Some(primary)) => vec![primary],
        _ => vec![UNSPECIFIED],
    }
}

/// The `Language` attribute value: hexadecimal LANGIDs separated by semicolons, e.g. `809;9`.
pub fn attribute(langids: &[u16]) -> String {
    langids
        .iter()
        .map(|langid| format!("{langid:x}"))
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kurmanji_follows_the_policy() {
        assert_eq!(
            attribute(&langids("ku_TR", KurdishPolicy::Unspecified)),
            "1000"
        );
        assert_eq!(
            attribute(&langids(
                "kmr-Latn",
                KurdishPolicy::CentralKurdishThenUnspecified
            )),
            "492;1000"
        );
        assert_eq!(
            attribute(&langids("ku_TR", KurdishPolicy::default())),
            "492;1000"
        );
        assert_eq!(
            attribute(&langids("ku", KurdishPolicy::CentralKurdish)),
            "492"
        );
    }

    #[test]
    fn known_languages_list_the_region_then_the_primary_language() {
        assert_eq!(
            attribute(&langids("en_GB", KurdishPolicy::default())),
            "809;9"
        );
        assert_eq!(
            attribute(&langids("tr-TR", KurdishPolicy::default())),
            "41f;1f"
        );
        assert_eq!(attribute(&langids("pt_AO", KurdishPolicy::default())), "16");
    }

    #[test]
    fn unknown_languages_are_unspecified() {
        assert_eq!(langids("xx_YY", KurdishPolicy::default()), [UNSPECIFIED]);
        assert_eq!(langids("", KurdishPolicy::default()), [UNSPECIFIED]);
    }

    #[test]
    fn policies_parse_from_their_names_and_round_trip() {
        for policy in [
            KurdishPolicy::Unspecified,
            KurdishPolicy::CentralKurdishThenUnspecified,
            KurdishPolicy::CentralKurdish,
        ] {
            assert_eq!(policy.to_string().parse::<KurdishPolicy>(), Ok(policy));
        }
        assert_eq!("492".parse(), Ok(KurdishPolicy::CentralKurdish));
        assert_eq!(
            "492;1000".parse(),
            Ok(KurdishPolicy::CentralKurdishThenUnspecified)
        );
        assert!("sorani".parse::<KurdishPolicy>().is_err());
    }
}
