use crate::letters::{word_to_segments, Segment};
use crate::normalize::normalize;
use crate::suffix::{strip_unstressed_suffixes, unstressed_suffix_count};

const PRIMARY: char = 'ˈ';
const SECONDARY: char = 'ˌ';

const UNSTRESSED: &[&str] = &[
    "an", "ên", "ya", "yê", "yan", "yên", "ji", "bi", "re", "di", "de", "kir", "got",
];

const UNSTRESSED_UNLESS_CLAUSE_FINAL: &[&str] = &[
    "û", "a", "ê", "ez", "te", "wê", "wî", "me", "we", "wan", "tu", "ew", "em", "hûn", "lê",
];

const FIRST_SYLLABLE: &[&str] = &[
    "nebû",
    "naxwaze",
    "nabînim",
    "nizanim",
    "nabêjim",
    "meke",
    "mexwe",
    "bike",
    "bixwe",
    "bixwînim",
    "vekirin",
    "veke",
    "rabûn",
    "radibim",
    "radibe",
    "hilbijartin",
    "hildibijêrim",
    "daketin",
    "dakeve",
    "derketin",
    "derket",
];

const LETTER_NAMES: &[(char, &str, Option<&str>)] = &[
    ('b', "b", Some("e")),
    ('c', "dʒ", Some("e")),
    ('ç', "tʃ", Some("e")),
    ('d', "d", Some("ɛ")),
    ('f', "f", Some("e")),
    ('g', "ɡ", Some("e")),
    ('h', "h", Some("e")),
    ('j', "ʒ", Some("e")),
    ('k', "k", Some("a")),
    ('l', "l", Some("e")),
    ('m', "m", Some("e")),
    ('n', "n", Some("e")),
    ('p', "pʰ", None),
    ('q', "q", None),
    ('r', "r", Some("e")),
    ('s', "s", Some("e")),
    ('ş', "ʃ", Some("e")),
    ('t', "t", Some("ˈɛ")),
    ('v', "v", Some("e")),
    ('w', "w", Some("e")),
    ('x', "x", Some("e")),
    ('z', "z", Some("e")),
];

const OVERRIDES: &[(&str, &str)] = &[("anji", "anʒˌɪ"), ("hwd", "hˌɛr wɛki dˈɪn")];

type Marks = Vec<(usize, char)>;

fn stem_nuclei(word: &str, nuclei: usize) -> usize {
    nuclei.saturating_sub(unstressed_suffix_count(word))
}

fn default_marks(stem: usize) -> Marks {
    let Some(last) = stem.checked_sub(1) else {
        return Vec::new();
    };
    (0..last.saturating_sub(1))
        .step_by(2)
        .map(|i| (i, SECONDARY))
        .chain(std::iter::once((last, PRIMARY)))
        .collect()
}

fn first_syllable_marks(nuclei: usize) -> Marks {
    (0..nuclei)
        .step_by(2)
        .map(|i| (i, if i == 0 { PRIMARY } else { SECONDARY }))
        .collect()
}

fn is_unstressed(word: &str, clause_final: bool) -> bool {
    UNSTRESSED.contains(&word) || (!clause_final && UNSTRESSED_UNLESS_CLAUSE_FINAL.contains(&word))
}

fn stress_marks(word: &str, nuclei: usize, clause_final: bool) -> Marks {
    if is_unstressed(word, clause_final) {
        Vec::new()
    } else if FIRST_SYLLABLE.contains(&strip_unstressed_suffixes(word).as_str()) {
        first_syllable_marks(stem_nuclei(word, nuclei))
    } else {
        default_marks(stem_nuclei(word, nuclei))
    }
}

fn render(segments: &[Segment], marks: &Marks) -> String {
    // `marks` stays sorted by nucleus index, which the binary search relies on.
    let mark_at = |nucleus: usize| {
        marks
            .binary_search_by_key(&nucleus, |(i, _)| *i)
            .ok()
            .and_then(|k| marks.get(k))
            .map(|(_, m)| *m)
    };
    segments
        .iter()
        .scan(0_usize, |seen, segment| {
            let nucleus = segment.is_vowel.then_some(*seen);
            *seen += usize::from(segment.is_vowel);
            let mark = nucleus.and_then(mark_at);
            Some(
                mark.into_iter()
                    .chain(segment.ipa.chars())
                    .collect::<String>(),
            )
        })
        .collect()
}

fn letter_name(letter: char) -> Option<(&'static str, Option<&'static str>)> {
    LETTER_NAMES
        .iter()
        .find(|(l, _, _)| *l == letter)
        .map(|(_, onset, vowel)| (*onset, *vowel))
}

fn spell_letters(word: &str) -> Option<String> {
    let names: Vec<_> = word.chars().map(letter_name).collect::<Option<_>>()?;
    let last_voiced = names.iter().rposition(|(_, vowel)| vowel.is_some());
    let spelled = names
        .iter()
        .enumerate()
        .map(|(i, (onset, vowel))| match vowel {
            Some(v) if v.starts_with(PRIMARY) => format!("{onset}{v}"),
            Some(v) if Some(i) == last_voiced => format!("{onset}{PRIMARY}{v}"),
            Some(v) => format!("{onset}{SECONDARY}{v}"),
            None => (*onset).to_string(),
        })
        .collect();
    Some(spelled)
}

/// IPA for one word. `clause_final` restores stress on pronouns and particles;
/// callers set it for the last token of a clause that has more than one token.
pub(crate) fn word_to_ipa(word: &str, clause_final: bool) -> String {
    let word = normalize(word);
    if let Some((_, ipa)) = OVERRIDES.iter().find(|(w, _)| *w == word) {
        return (*ipa).to_string();
    }
    if let Some(spelled) = spell_letters(&word) {
        return spelled;
    }
    let segments = word_to_segments(&word);
    let nuclei = segments.iter().filter(|s| s.is_vowel).count();
    render(&segments, &stress_marks(&word, nuclei, clause_final))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_marks_stress_the_last_nucleus() {
        assert_eq!(default_marks(1), vec![(0, PRIMARY)]);
        assert_eq!(default_marks(2), vec![(1, PRIMARY)]);
    }

    #[test]
    fn default_marks_add_secondary_on_odd_syllables_not_next_to_the_primary() {
        assert_eq!(default_marks(3), vec![(0, SECONDARY), (2, PRIMARY)]);
        assert_eq!(default_marks(4), vec![(0, SECONDARY), (3, PRIMARY)]);
        assert_eq!(
            default_marks(5),
            vec![(0, SECONDARY), (2, SECONDARY), (4, PRIMARY)]
        );
        assert_eq!(
            default_marks(6),
            vec![(0, SECONDARY), (2, SECONDARY), (5, PRIMARY)]
        );
    }

    #[test]
    fn first_syllable_marks_alternate_from_the_third_nucleus() {
        assert_eq!(first_syllable_marks(2), vec![(0, PRIMARY)]);
        assert_eq!(
            first_syllable_marks(5),
            vec![(0, PRIMARY), (2, SECONDARY), (4, SECONDARY)]
        );
    }

    #[test]
    fn a_single_vowel_word_keeps_its_stress_despite_a_suffix_lookalike() {
        assert_eq!(word_to_ipa("ba", false), "bˈa");
    }

    #[test]
    fn consonant_only_words_are_spelled_by_letter_name() {
        assert_eq!(word_to_ipa("bsk", false), "bˌesˌekˈa");
        assert_eq!(word_to_ipa("Dr", false), "dˌɛrˈe");
        assert_eq!(word_to_ipa("p", false), "pʰ");
    }

    #[test]
    fn a_very_long_word_of_many_vowels_is_stressed_in_linear_time() {
        let ipa = word_to_ipa(&"i".repeat(300_000), false);
        assert!(ipa.starts_with("ˌɪ"));
        assert!(ipa.ends_with("ˈɪ"));
    }

    #[test]
    fn consonant_words_with_y_are_not_spelled() {
        assert_eq!(word_to_ipa("xyz", false), "xjz");
    }

    #[test]
    fn pronouns_regain_stress_at_the_end_of_a_clause() {
        assert_eq!(word_to_ipa("ez", false), "ɛz");
        assert_eq!(word_to_ipa("ez", true), "ˈɛz");
        assert_eq!(word_to_ipa("bi", true), "bɪ");
    }
}
