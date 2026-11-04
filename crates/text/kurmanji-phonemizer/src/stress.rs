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
    ('t', "t", Some("ɛ")),
    ('v', "v", Some("e")),
    ('w', "w", Some("e")),
    ('x', "x", Some("e")),
    ('z', "z", Some("e")),
];

const ACRONYM_ONLY_NAMES: &[(char, &str, &str)] = &[
    ('a', "", "a"),
    ('e', "", "ɛ"),
    ('ê', "", "e"),
    ('i', "", "ɪ"),
    ('î', "", "i"),
    ('o', "", "o"),
    ('u', "", "u"),
    ('û', "", "u"),
    ('y', "j", "e"),
];

const SPELLING_GROUP: usize = 3;
const MAX_UNSPLIT_LETTERS: usize = 4;

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

fn is_listed_stem(stem: &str, clause_final: bool) -> bool {
    UNSTRESSED.contains(&stem) || (UNSTRESSED_UNLESS_CLAUSE_FINAL.contains(&stem) && !clause_final)
}

/// One unstressed suffix on a listed stem marks the first vowel secondary; plural
/// `-an` marks only the last vowel secondary. `None` when the stem is unlisted, or
/// is a clause-final pronoun that regains full stress.
fn listed_stem_marks(word: &str, nuclei: usize, clause_final: bool) -> Option<Marks> {
    let stem = strip_unstressed_suffixes(word);
    let suffix_marks = || {
        (unstressed_suffix_count(word) == 1 && is_listed_stem(&stem, clause_final))
            .then(|| vec![(0, SECONDARY)])
    };
    let plural_marks = || {
        let plural_stem = word.strip_suffix("an")?;
        is_listed_stem(plural_stem, clause_final)
            .then(|| vec![(nuclei.saturating_sub(1), SECONDARY)])
    };
    suffix_marks().or_else(plural_marks)
}

fn is_first_syllable_stem(word: &str) -> bool {
    let stem = strip_unstressed_suffixes(word);
    FIRST_SYLLABLE.contains(&stem.as_str())
        || stem
            .strip_suffix("an")
            .is_some_and(|plural| FIRST_SYLLABLE.contains(&plural))
}

fn stress_marks(word: &str, nuclei: usize, clause_final: bool) -> Marks {
    if is_unstressed(word, clause_final) {
        Vec::new()
    } else if let Some(marks) = listed_stem_marks(word, nuclei, clause_final) {
        marks
    } else if is_first_syllable_stem(word) {
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

fn letter_name(letter: char, with_vowels: bool) -> Option<LetterName> {
    LETTER_NAMES
        .iter()
        .find(|(l, _, _)| *l == letter)
        .map(|(_, onset, vowel)| (*onset, *vowel))
        .or_else(|| {
            ACRONYM_ONLY_NAMES
                .iter()
                .filter(|_| with_vowels)
                .find(|(l, _, _)| *l == letter)
                .map(|(_, onset, vowel)| (*onset, Some(*vowel)))
        })
}

type LetterName = (&'static str, Option<&'static str>);

/// eSpeak-ng spells a long letter run in groups of three, each with its own
/// final stress, and leaves a remainder of up to four letters whole.
fn spelling_groups(names: &[LetterName]) -> Vec<&[LetterName]> {
    let splits = names
        .len()
        .saturating_sub(MAX_UNSPLIT_LETTERS)
        .div_ceil(SPELLING_GROUP);
    let (heads, tail) = names.split_at(splits * SPELLING_GROUP);
    heads
        .chunks(SPELLING_GROUP)
        .chain(std::iter::once(tail))
        .collect()
}

fn spell_group(names: &[LetterName]) -> String {
    let last_voiced = names.iter().rposition(|(_, vowel)| vowel.is_some());
    names
        .iter()
        .enumerate()
        .map(|(i, (onset, vowel))| match vowel {
            Some(v) if Some(i) == last_voiced => format!("{onset}{PRIMARY}{v}"),
            Some(v) => format!("{onset}{SECONDARY}{v}"),
            None => (*onset).to_string(),
        })
        .collect()
}

fn spell_letters(word: &str, with_vowels: bool) -> Option<String> {
    let names: Vec<_> = word
        .chars()
        .map(|letter| letter_name(letter, with_vowels))
        .collect::<Option<_>>()?;
    Some(
        spelling_groups(&names)
            .into_iter()
            .map(spell_group)
            .collect(),
    )
}

/// Spells `word` letter by letter with vowel letter names, or `None` if any letter has no name.
pub(crate) fn acronym_to_ipa(word: &str) -> Option<String> {
    spell_letters(&normalize(word), true)
}

fn rules_to_ipa(word: &str) -> String {
    let segments = word_to_segments(word);
    let nuclei = segments.iter().filter(|s| s.is_vowel).count();
    render(&segments, &default_marks(stem_nuclei(word, nuclei)))
}

/// A consonant-only start before an apostrophe (`tv'ya`) is spelled out and
/// the rest read by the plain letter rules, as eSpeak-ng does.
fn spelled_prefix_split(word: &str) -> Option<String> {
    let (prefix, rest) = word.split_once('\'')?;
    let spelled = spell_letters(prefix, false).filter(|_| prefix.chars().count() > 1)?;
    Some(format!("{spelled}{}", rules_to_ipa(rest)))
}

/// IPA for one word. `clause_final` restores stress on pronouns and particles;
/// callers set it for the last token of a clause that has more than one token.
pub(crate) fn word_to_ipa(word: &str, clause_final: bool) -> String {
    let word = normalize(word);
    if let Some((_, ipa)) = OVERRIDES.iter().find(|(w, _)| *w == word) {
        return (*ipa).to_string();
    }
    if let Some(spelled) = spell_letters(&word, false).or_else(|| spelled_prefix_split(&word)) {
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

    #[test]
    fn a_listed_stem_with_one_suffix_is_secondary() {
        assert_eq!(word_to_ipa("wanê", false), "wˌane");
        assert_eq!(word_to_ipa("kirê", false), "kˌɪre");
        assert_eq!(word_to_ipa("wanêê", false), "wˈanee");
    }

    #[test]
    fn clause_final_restores_only_the_plus_flagged_stems() {
        assert_eq!(word_to_ipa("wanê", true), "wˈane");
        assert_eq!(word_to_ipa("kirê", true), "kˌɪre");
    }

    #[test]
    fn plural_an_after_a_listed_stem_is_secondary() {
        assert_eq!(word_to_ipa("wanan", false), "wanˌan");
        assert_eq!(word_to_ipa("dostan", false), "dostˈan");
    }

    #[test]
    fn first_syllable_stems_keep_their_stress_before_plural_an() {
        assert_eq!(word_to_ipa("hilbijartinan", false), "hˈɪlbɪʒˌartɪnˌan");
    }

    #[test]
    fn long_letter_runs_are_spelled_in_groups_of_three() {
        assert_eq!(word_to_ipa("bdtvw", false), "bˌedˌɛtˈɛvˌewˈe");
        assert_eq!(word_to_ipa("bdtvwz", false), "bˌedˌɛtˈɛvˌewˌezˈe");
        assert_eq!(word_to_ipa("bdtvwzx", false), "bˌedˌɛtˈɛvˌewˌezˌexˈe");
    }

    #[test]
    fn a_spelled_prefix_before_an_apostrophe_leaves_the_rest_to_the_rules() {
        assert_eq!(word_to_ipa("tv'ya", false), "tˌɛvˈejˈa");
    }

    #[test]
    fn an_apostrophe_keeps_r_from_flapping() {
        assert_eq!(word_to_ipa("Qur’anê", false), "qʊrˈane");
    }

    #[test]
    fn acronyms_are_spelled_with_vowel_letter_names() {
        assert_eq!(acronym_to_ipa("ISO").as_deref(), Some("ˌɪsˌeˈo"));
        assert_eq!(acronym_to_ipa("YB").as_deref(), Some("jˌebˈe"));
        assert_eq!(acronym_to_ipa("ÖB"), None);
    }
}
