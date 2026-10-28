const AND: &str = "û";
const VOWELS: &str = "aeêiîouû";
const MAX_SPELLED: usize = 999_999_999;

const UNITS_AND_TEENS: [&str; 20] = [
    "sifir", "yek", "du", "sê", "çar", "pênc", "şeş", "heft", "heşt", "neh", "deh", "yazde",
    "diwanzde", "sêzde", "çarde", "panzde", "şanzde", "hivdeh", "hijdeh", "nozdeh",
];

const TENS: [&str; 8] = [
    "bîst", "sî", "çil", "pêncî", "şêst", "heftê", "heştê", "nod",
];

fn word(list: &[&'static str], index: usize) -> &'static str {
    list.get(index).copied().unwrap_or_default()
}

fn below_hundred(value: usize) -> Vec<&'static str> {
    let (tens, units) = (value / 10, value % 10);
    match (tens, units) {
        (0 | 1, _) => vec![word(&UNITS_AND_TEENS, value)],
        (_, 0) => vec![word(&TENS, tens - 2)],
        _ => vec![word(&TENS, tens - 2), AND, word(&UNITS_AND_TEENS, units)],
    }
}

fn below_thousand(value: usize) -> Vec<&'static str> {
    let hundreds = match value / 100 {
        0 => Vec::new(),
        1 => vec!["sed"],
        h => vec![word(&UNITS_AND_TEENS, h), "sed"],
    };
    let rest = match value % 100 {
        0 => Vec::new(),
        r if hundreds.is_empty() => below_hundred(r),
        r => std::iter::once(AND).chain(below_hundred(r)).collect(),
    };
    hundreds.into_iter().chain(rest).collect()
}

fn scaled(value: usize, scale: usize, name: &'static str) -> Vec<&'static str> {
    match value / scale % 1000 {
        0 => Vec::new(),
        1 => vec![name],
        n => below_thousand(n).into_iter().chain([name]).collect(),
    }
}

fn join_groups(groups: [Vec<&'static str>; 3]) -> Vec<&'static str> {
    groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .enumerate()
        .flat_map(|(i, g)| {
            let and = (i > 0).then_some(AND);
            and.into_iter().chain(g)
        })
        .collect()
}

pub(crate) fn cardinal_words(value: usize) -> Option<Vec<&'static str>> {
    if value > MAX_SPELLED {
        return None;
    }
    if value == 0 {
        return Some(vec![word(&UNITS_AND_TEENS, 0)]);
    }
    Some(join_groups([
        scaled(value, 1_000_000, "milyon"),
        scaled(value, 1_000, "hezar"),
        below_thousand(value % 1000),
    ]))
}

pub(crate) fn with_suffix(last: &str, consonant_form: &str, vowel_form: &str) -> String {
    let ends_in_vowel = last.chars().next_back().is_some_and(|c| VOWELS.contains(c));
    format!(
        "{last}{}",
        if ends_in_vowel {
            vowel_form
        } else {
            consonant_form
        }
    )
}

pub(crate) fn ordinal_words(value: usize, formal: bool) -> Option<Vec<String>> {
    let mut words: Vec<String> = cardinal_words(value)?
        .into_iter()
        .map(str::to_string)
        .collect();
    let last = words.pop()?;
    let ordinal = if formal {
        with_suffix(&last, "emîn", "yemîn")
    } else {
        with_suffix(&last, "em", "yem")
    };
    words.push(ordinal);
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(value: usize) -> String {
        cardinal_words(value).unwrap_or_default().join(" ")
    }

    #[test]
    fn spells_small_numbers() {
        assert_eq!(spoken(0), "sifir");
        assert_eq!(spoken(4), "çar");
        assert_eq!(spoken(19), "nozdeh");
        assert_eq!(spoken(28), "bîst û heşt");
        assert_eq!(spoken(80), "heştê");
    }

    #[test]
    fn spells_hundreds_thousands_and_millions() {
        assert_eq!(spoken(100), "sed");
        assert_eq!(spoken(101), "sed û yek");
        assert_eq!(spoken(200), "du sed");
        assert_eq!(spoken(1000), "hezar");
        assert_eq!(spoken(2026), "du hezar û bîst û şeş");
        assert_eq!(spoken(1_000_001), "milyon û yek");
        assert_eq!(spoken(2_500_000), "du milyon û pênc sed hezar");
    }

    #[test]
    fn refuses_numbers_beyond_the_spelled_range() {
        assert!(cardinal_words(1_000_000_000).is_none());
    }

    #[test]
    fn ordinals_add_em_or_yem_to_the_last_word() {
        let ordinal = |n| ordinal_words(n, false).unwrap_or_default().join(" ");
        assert_eq!(ordinal(1), "yekem");
        assert_eq!(ordinal(2), "duyem");
        assert_eq!(ordinal(3), "sêyem");
        assert_eq!(ordinal(4), "çarem");
        assert_eq!(ordinal(12), "diwanzdeyem");
        assert_eq!(ordinal(20), "bîstem");
        assert_eq!(ordinal(21), "bîst û yekem");
        assert_eq!(ordinal(100), "sedem");
        assert_eq!(
            ordinal_words(3, true).unwrap_or_default().join(" "),
            "sêyemîn"
        );
    }
}
