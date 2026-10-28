use unicode_normalization::UnicodeNormalization;

fn is_hawar_letter(c: char) -> bool {
    c.is_ascii_lowercase() || matches!(c, 'î' | 'ê' | 'û' | 'ç' | 'ş' | 'ö' | 'ü')
}

fn base_letter(c: char) -> Option<char> {
    match c {
        'ı' => Some('i'),
        'ł' => Some('l'),
        'ø' => Some('o'),
        _ => std::iter::once(c)
            .nfd()
            .next()
            .filter(char::is_ascii_lowercase),
    }
}

fn fold_to_hawar(c: char) -> Option<char> {
    if is_hawar_letter(c) {
        Some(c)
    } else if c.is_alphabetic() {
        base_letter(c)
    } else {
        None
    }
}

pub(crate) fn normalize(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .nfc()
        .filter_map(fold_to_hawar)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposed_equals_precomposed() {
        assert_eq!(normalize("i\u{302}"), "î");
        assert_eq!(normalize("s\u{327}"), "ş");
    }

    #[test]
    fn uppercase_hawar_letters_lower_correctly() {
        assert_eq!(normalize("ÎÊÛÇŞI"), "îêûçşi");
    }

    #[test]
    fn foreign_latin_letters_fold_to_their_base_letter() {
        assert_eq!(normalize("Éric"), "eric");
        assert_eq!(normalize("Çoğürlük"), "çogürlük");
        assert_eq!(normalize("Diyarbakır"), "diyarbakir");
        assert_eq!(normalize("Łódź"), "lodz");
    }

    #[test]
    fn non_latin_scripts_and_punctuation_are_dropped() {
        assert_eq!(normalize("سلاو-1"), "");
    }
}
