use std::collections::HashMap;

pub(crate) const PINYIN_INITIALS: [&str; 23] = [
    "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "r",
    "z", "c", "s", "y", "w",
];

pub(crate) fn convert_bopomofo_to_pinyin(
    bopomofo: &str,
    dict: &HashMap<String, String>,
) -> Option<String> {
    let mut chars: Vec<char> = bopomofo.chars().collect();
    let tone = chars.pop()?;
    if !tone.is_ascii_digit() {
        return None;
    }
    let component: String = chars.into_iter().collect();
    dict.get(&component).map(|pinyin| format!("{pinyin}{tone}"))
}

pub(crate) fn split_initial_final_tone(syllable: &str) -> Option<(String, String, char)> {
    let mut chars: Vec<char> = syllable.chars().collect();
    let tone = chars.pop()?;
    if !tone.is_ascii_digit() || tone == '0' {
        return None;
    }
    let base: String = chars.into_iter().collect();
    if base.is_empty() || !base.chars().all(|c| c.is_ascii_lowercase() || c == 'v') {
        return None;
    }

    let initial = PINYIN_INITIALS
        .iter()
        .find(|cand| base.starts_with(**cand))
        .copied()
        .unwrap_or("");
    let finale = base[initial.len()..].to_string();
    Some((initial.to_string(), finale, tone))
}

#[cfg(test)]
mod tests {
    #[test]
    fn convert_bopomofo_to_pinyin_returns_none_for_a_non_digit_tone_mark() {
        let dict: std::collections::HashMap<String, String> =
            [("ㄏㄤ".to_string(), "hang".to_string())]
                .into_iter()
                .collect();
        assert_eq!(super::convert_bopomofo_to_pinyin("ㄏㄤˊ", &dict), None);
    }

    #[test]
    fn convert_bopomofo_to_pinyin_appends_the_original_tone_digit() {
        let dict: std::collections::HashMap<String, String> =
            [("ㄏㄤ".to_string(), "hang".to_string())]
                .into_iter()
                .collect();
        assert_eq!(
            super::convert_bopomofo_to_pinyin("ㄏㄤ2", &dict),
            Some("hang2".to_string())
        );
    }

    #[test]
    fn convert_bopomofo_to_pinyin_returns_none_for_an_unknown_component() {
        let dict = std::collections::HashMap::new();
        assert_eq!(super::convert_bopomofo_to_pinyin("ㄏㄤ2", &dict), None);
    }

    #[test]
    fn split_initial_final_tone_matches_the_longest_initial_first() {
        assert_eq!(
            super::split_initial_final_tone("zhang2"),
            Some(("zh".to_string(), "ang".to_string(), '2'))
        );
        assert_ne!(
            super::split_initial_final_tone("zhang2").map(|(i, _, _)| i),
            Some("z".to_string())
        );
    }

    #[test]
    fn split_initial_final_tone_handles_a_zero_initial_syllable() {
        assert_eq!(
            super::split_initial_final_tone("ai3"),
            Some(("".to_string(), "ai".to_string(), '3'))
        );
    }

    #[test]
    fn split_initial_final_tone_returns_none_for_a_non_pinyin_string() {
        assert_eq!(super::split_initial_final_tone("hello"), None);
        assert_eq!(super::split_initial_final_tone(""), None);
    }
}
