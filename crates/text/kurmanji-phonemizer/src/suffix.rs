const UNSTRESSED_SUFFIXES: [&[char]; 3] = [&['ê', 'n'], &['ê'], &['a']];
const CONTEXT_SUFFIXES: [&[char]; 4] = [&['ê', 'n'], &['a', 'n'], &['ê'], &['a']];

pub(crate) fn is_vowel_letter(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'ê' | 'i' | 'î' | 'o' | 'u' | 'û' | 'ö' | 'ü')
}

fn vowel_count(chars: &[char]) -> usize {
    chars.iter().filter(|c| is_vowel_letter(**c)).count()
}

fn suffix_len(chars: &[char], suffixes: &[&[char]]) -> usize {
    suffixes
        .iter()
        .find(|suffix| vowel_count(chars) >= 2 && chars.ends_with(suffix))
        .map_or(0, |suffix| suffix.len())
}

/// Stem lengths, in chars, after each successive unstressed suffix is peeled
/// off; the first entry is the full word (`mavênê` is `mav` + `ên` + `ê`).
fn peeled_stem_lengths(chars: &[char]) -> Vec<usize> {
    let start = (chars.len(), vowel_count(chars));
    std::iter::successors(Some(start), |&(len, vowels)| {
        let stem = chars.get(..len)?;
        let suffix = UNSTRESSED_SUFFIXES
            .iter()
            .find(|suffix| vowels >= 2 && stem.ends_with(suffix))?;
        Some((len - suffix.len(), vowels - 1))
    })
    .map(|(len, _)| len)
    .collect()
}

pub(crate) fn unstressed_suffix_count(word: &str) -> usize {
    let chars: Vec<char> = word.chars().collect();
    peeled_stem_lengths(&chars).len() - 1
}

pub(crate) fn strip_unstressed_suffixes(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let stem_len = peeled_stem_lengths(&chars).last().copied().unwrap_or(0);
    chars.iter().take(stem_len).collect()
}

/// Length in chars of the single suffix espeak removes before applying
/// letter-context rules (`r`, `g`, `k`), plural `-an` included.
pub(crate) fn context_suffix_len(word: &str) -> usize {
    let chars: Vec<char> = word.chars().collect();
    suffix_len(&chars, &CONTEXT_SUFFIXES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unstressed_suffixes_peel_off_repeatedly() {
        assert_eq!(unstressed_suffix_count("mavênê"), 2);
        assert_eq!(unstressed_suffix_count("mazraê"), 2);
        assert_eq!(unstressed_suffix_count("zimanê"), 1);
        assert_eq!(unstressed_suffix_count("mala"), 1);
        assert_eq!(unstressed_suffix_count("malên"), 1);
    }

    #[test]
    fn suffix_needs_a_vowel_before_it() {
        assert_eq!(unstressed_suffix_count("ba"), 0);
    }

    #[test]
    fn plural_an_is_not_an_unstressed_suffix() {
        assert_eq!(unstressed_suffix_count("dostan"), 0);
    }

    #[test]
    fn stripping_returns_the_bare_stem() {
        assert_eq!(strip_unstressed_suffixes("mavênê"), "mav");
        assert_eq!(strip_unstressed_suffixes("hilbijartinên"), "hilbijartin");
        assert_eq!(strip_unstressed_suffixes("ba"), "ba");
    }

    #[test]
    fn a_very_long_word_does_not_overflow_the_stack() {
        let word = "a".repeat(500_000);
        assert_eq!(unstressed_suffix_count(&word), 499_999);
        assert_eq!(strip_unstressed_suffixes(&word), "a");
    }

    #[test]
    fn plural_an_is_a_context_suffix() {
        assert_eq!(context_suffix_len("dostan"), 2);
        assert_eq!(context_suffix_len("nan"), 0);
    }

    #[test]
    fn only_the_outermost_context_suffix_is_removed() {
        assert_eq!(context_suffix_len("baranê"), 1);
    }
}
