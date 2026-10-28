use crate::suffix::{context_suffix_len, is_vowel_letter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    pub ipa: &'static str,
    pub is_vowel: bool,
}

const fn seg(ipa: &'static str, is_vowel: bool) -> Segment {
    Segment { ipa, is_vowel }
}

const DIGRAPHS: &[(&str, &[Segment])] = &[
    ("xwe", &[seg("xw", false), seg("ɵ", true)]),
    ("rr", &[seg("r", false)]),
    ("ew", &[seg("eʊ", true)]),
    ("ey", &[seg("eɪ", true)]),
];

const SINGLES: &[(char, Segment)] = &[
    ('a', seg("a", true)),
    ('b', seg("b", false)),
    ('c', seg("dʒ", false)),
    ('ç', seg("tʃ", false)),
    ('d', seg("d", false)),
    ('e', seg("ɛ", true)),
    ('ê', seg("e", true)),
    ('f', seg("f", false)),
    ('g', seg("ɡ", false)),
    ('h', seg("h", false)),
    ('i', seg("ɪ", true)),
    ('î', seg("i", true)),
    ('j', seg("ʒ", false)),
    ('k', seg("k", false)),
    ('l', seg("l", false)),
    ('m', seg("m", false)),
    ('n', seg("n", false)),
    ('o', seg("o", true)),
    ('ö', seg("ɪː", true)),
    ('p', seg("p", false)),
    ('q', seg("q", false)),
    ('r', seg("r", false)),
    ('s', seg("s", false)),
    ('ş', seg("ʃ", false)),
    ('t', seg("t", false)),
    ('u', seg("ʊ", true)),
    ('û', seg("u", true)),
    ('ü', seg("y", true)),
    ('v', seg("v", false)),
    ('w', seg("w", false)),
    ('x', seg("x", false)),
    ('y', seg("j", false)),
    ('z', seg("z", false)),
];

const FRONT_VOWEL_STARTS: [&str; 5] = ["î", "ê", "iy", "ew", "ey"];

fn starts_with(rest: &[char], pattern: &str) -> bool {
    rest.iter()
        .copied()
        .take(pattern.chars().count())
        .eq(pattern.chars())
}

fn front_vowel_follows(rest: &[char]) -> bool {
    FRONT_VOWEL_STARTS.iter().any(|p| starts_with(rest, p))
}

fn is_flapped_r(prev: Option<char>, rest: &[char]) -> bool {
    prev.is_some_and(is_vowel_letter) && rest.get(1).copied().is_some_and(is_vowel_letter)
}

fn context_segment(prev: Option<char>, rest: &[char]) -> Option<Segment> {
    match rest.first()? {
        'g' if front_vowel_follows(rest.get(1..)?) => Some(seg("ɟ", false)),
        'k' if front_vowel_follows(rest.get(1..)?) => Some(seg("c", false)),
        'r' if is_flapped_r(prev, rest) => Some(seg("ɾ", false)),
        _ => None,
    }
}

fn digraph_segments(rest: &[char]) -> Option<(&'static [Segment], usize)> {
    DIGRAPHS
        .iter()
        .find(|(pattern, _)| starts_with(rest, pattern))
        .map(|(pattern, segments)| (*segments, pattern.chars().count()))
}

fn single_segment(c: char) -> Option<Segment> {
    SINGLES.iter().find(|(l, _)| *l == c).map(|(_, s)| *s)
}

fn next_segments(prev: Option<char>, rest: &[char], context: &[char]) -> (Vec<Segment>, usize) {
    if let Some((segments, used)) = digraph_segments(rest) {
        return (segments.to_vec(), used);
    }
    let found =
        context_segment(prev, context).or_else(|| rest.first().copied().and_then(single_segment));
    (found.into_iter().collect(), 1)
}

pub(crate) fn word_to_segments(word: &str) -> Vec<Segment> {
    let chars: Vec<char> = word.chars().collect();
    let stem_end = chars.len() - context_suffix_len(word);
    let (segments, _) = (0..chars.len()).fold((Vec::new(), 0), |(mut out, resume), i| {
        let rest = chars.get(i..).unwrap_or_default();
        if i < resume || rest.is_empty() {
            return (out, resume);
        }
        let prev = i.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let context = chars.get(i..stem_end).filter(|c| !c.is_empty());
        let (found, used) = next_segments(prev, rest, context.unwrap_or(rest));
        out.extend(found);
        (out, i + used)
    });
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;

    const GOLDEN: &str = include_str!("../tests/golden_words.tsv");

    fn without_stress(ipa: &str) -> String {
        ipa.chars().filter(|c| !matches!(c, 'ˈ' | 'ˌ')).collect()
    }

    fn joined(word: &str) -> String {
        word_to_segments(&normalize(word))
            .iter()
            .map(|s| s.ipa)
            .collect()
    }

    #[test]
    fn letters_match_espeak_before_stress() {
        let mismatches: Vec<String> = GOLDEN
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .filter(|(word, ipa)| joined(word) != without_stress(ipa))
            .map(|(word, ipa)| format!("{word}: got {} want {}", joined(word), without_stress(ipa)))
            .collect();
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }

    #[test]
    fn r_flaps_between_vowels_inside_the_stem_only() {
        assert_eq!(joined("pori"), "poɾɪ");
        assert_eq!(joined("paru"), "paɾʊ");
        assert_eq!(joined("bareb"), "baɾɛb");
        assert_eq!(joined("para"), "para");
        assert_eq!(joined("parê"), "pare");
        assert_eq!(joined("pro"), "pro");
    }

    #[test]
    fn r_before_the_plural_suffix_is_a_trill_but_flaps_before_a_stripped_outer_suffix() {
        assert_eq!(joined("baran"), "baran");
        assert_eq!(joined("baranê"), "baɾane");
    }

    #[test]
    fn unknown_characters_are_dropped() {
        assert_eq!(joined("a1ب-b"), "ab");
    }
}
