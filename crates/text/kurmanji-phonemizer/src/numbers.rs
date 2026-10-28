const END: char = '|';
const AND: &str = "u";
const PRIMARY_STRESS: char = 'ˈ';
const HUNDRED: &str = "sˈɛd";
const THOUSAND: &str = "hɛzˈar";
const MILLION: &str = "mɪljˈon";
const DECIMAL_POINT: &str = "xal";
const MAX_GROUPED_DIGITS: usize = 14;

const UNITS_AND_TEENS: [&str; 20] = [
    "sˈɪfr",
    "jˈɛk",
    "dˈʊ",
    "sˈe",
    "dʒˈar",
    "pˈendʒ",
    "ʃˈɛʃ",
    "hˈɛft",
    "hˈɛjʃt",
    "nˈɛh",
    "dˈɛh",
    "jazdˈɛ",
    "danzdˈɛ",
    "sezdˈɛ",
    "tʃardˈɛ",
    "panzdˈɛ",
    "ʃanzdˈɛ",
    "hɪvdˈɛh",
    "hɪʒdˈɛh",
    "nozdˈɛh",
];

const TENS: [&str; 8] = [
    "bˈɪst",
    "sˈi",
    "tʃˈɪl",
    "pendʒˈi",
    "ʃˈest",
    "hɛftˈe",
    "heʃtˈe",
    "nˈod",
];

fn digit_word(digit: usize) -> &'static str {
    UNITS_AND_TEENS.get(digit).copied().unwrap_or_default()
}

fn below_hundred(value: usize) -> String {
    match (value / 10, value % 10) {
        (0 | 1, _) => digit_word(value).to_string(),
        (tens, 0) => TENS.get(tens - 2).copied().unwrap_or_default().to_string(),
        (tens, units) => format!(
            "{}{AND}{}",
            TENS.get(tens - 2).copied().unwrap_or_default(),
            digit_word(units)
        ),
    }
}

fn magnitude_word(group_position: usize) -> &'static str {
    match group_position {
        1 | 3 => THOUSAND,
        _ => MILLION,
    }
}

fn has_exact_magnitude_word(value: usize, group_position: usize) -> bool {
    value == 1 && (1..=3).contains(&group_position)
}

fn thousands_prefix(thousands: usize) -> String {
    let digits = if has_exact_magnitude_word(thousands, 1) {
        String::new()
    } else {
        below_hundred(thousands)
    };
    format!("{digits}{END}{THOUSAND}{END}")
}

fn hundreds_block(hundreds: usize, after_higher_part: bool) -> (String, bool) {
    let and = if after_higher_part { AND } else { "" };
    match hundreds {
        0 => (String::new(), false),
        1 => (format!("{and}{HUNDRED}"), true),
        h => (format!("{and}{}{HUNDRED}", below_hundred(h)), true),
    }
}

fn three_digits(
    value: usize,
    after_higher_part: bool,
    group_position: usize,
    silent_zero: bool,
) -> String {
    let (prefix, higher) = match value / 1000 {
        0 => (String::new(), after_higher_part),
        thousands => (thousands_prefix(thousands), true),
    };
    let (hundreds, spoken_hundreds) = hundreds_block(value % 1000 / 100, higher);
    let tens_units = value % 100;
    let silent = silent_zero || value >= 1000 || spoken_hundreds;
    let and = tens_units > 0 && (value > 100 || (higher && group_position == 0));
    let tail = (tens_units != 0 || !silent).then(|| below_hundred(tens_units));
    let and = if and { AND } else { "" };
    format!("{prefix}{hundreds}{and}{END}{}", tail.unwrap_or_default())
}

fn groups_of_three(digits: &str) -> Vec<&str> {
    let first = match digits.len() % 3 {
        0 => 3,
        n => n,
    };
    let (head, tail) = digits.split_at(first);
    std::iter::once(head)
        .chain(
            tail.as_bytes()
                .chunks(3)
                .filter_map(|c| std::str::from_utf8(c).ok()),
        )
        .collect()
}

fn group_to_ipa(group: &str, position: usize, is_first: bool, total_groups: usize) -> String {
    let value: usize = group.parse().unwrap_or(0);
    let silent = value == 0 && !is_first;
    let exact = !silent && position > 0 && has_exact_magnitude_word(value, position);
    let missing_million = silent && position == 2 && total_groups >= 4;
    let appended = match (silent, position) {
        _ if missing_million => MILLION,
        (true, _) | (_, 0) => "",
        (false, p) => magnitude_word(p),
    };
    let spoken = if exact { 0 } else { value };
    let body = three_digits(spoken, !is_first, position, silent || exact);
    format!("{body}{END}{appended}{END}")
}

fn cardinal_core(digits: &str) -> String {
    if digits.len() <= 4 {
        return three_digits(digits.parse().unwrap_or(0), false, 0, false);
    }
    let groups = groups_of_three(digits);
    let last = groups.len() - 1;
    groups
        .iter()
        .enumerate()
        .map(|(i, g)| group_to_ipa(g, last - i, i == 0, groups.len()))
        .collect()
}

fn spoken_words(raw: &str) -> String {
    raw.split(END)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn digit_by_digit(digits: &str) -> String {
    digits
        .bytes()
        .map(|b| digit_word(usize::from(b.saturating_sub(b'0'))))
        .collect()
}

fn leading_zero_count(digits: &str) -> usize {
    let zeros = digits.bytes().take_while(|b| *b == b'0').count();
    zeros.min(digits.len().saturating_sub(1))
}

fn is_first_syllable_stressed(word: &str) -> bool {
    word.split_once(PRIMARY_STRESS)
        .is_some_and(|(onset, _)| !onset.chars().any(|c| "aeiouɛɪʊɵ".contains(c)))
}

fn stress_lone_and(spoken: &str) -> String {
    let words: Vec<&str> = spoken.split(' ').collect();
    words
        .iter()
        .enumerate()
        .map(|(i, word)| match words.get(i + 1) {
            Some(next) if *word == AND && !is_first_syllable_stressed(next) => "ˌu",
            _ => word,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn spelled_in_threes(digits: &str) -> String {
    groups_of_three(digits)
        .into_iter()
        .map(digit_by_digit)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn cardinal(digits: &str) -> String {
    if digits.len() > MAX_GROUPED_DIGITS {
        return spelled_in_threes(digits);
    }
    if digits.starts_with('0') && digits.len() > 3 {
        return digit_by_digit(digits);
    }
    let zero_words = digit_word(0).repeat(leading_zero_count(digits));
    let spoken = spoken_words(&format!("{zero_words}{END}{}", cardinal_core(digits)));
    if digits.len() > 4 {
        stress_lone_and(&spoken)
    } else {
        spoken
    }
}

pub(crate) fn decimal(integer: &str, fraction: &str) -> String {
    let digits = fraction
        .bytes()
        .map(|b| digit_word(usize::from(b.saturating_sub(b'0'))))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{} {DECIMAL_POINT} {digits}", cardinal(integer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_numbers_use_the_unit_and_teen_words() {
        assert_eq!(cardinal("0"), "sˈɪfr");
        assert_eq!(cardinal("19"), "nozdˈɛh");
    }

    #[test]
    fn tens_and_units_fuse_with_and() {
        assert_eq!(cardinal("21"), "bˈɪstujˈɛk");
        assert_eq!(cardinal("99"), "nˈodunˈɛh");
    }

    #[test]
    fn hundreds_follow_the_oracle() {
        assert_eq!(cardinal("100"), "sˈɛd");
        assert_eq!(cardinal("101"), "sˈɛdu jˈɛk");
        assert_eq!(cardinal("200"), "dˈʊsˈɛd");
        assert_eq!(cardinal("123"), "sˈɛdu bˈɪstusˈe");
    }

    #[test]
    fn thousands_and_millions_follow_the_oracle() {
        assert_eq!(cardinal("1000"), "hɛzˈar");
        assert_eq!(cardinal("2024"), "dˈʊ hɛzˈar u bˈɪstudʒˈar");
        assert_eq!(cardinal("12345"), "danzdˈɛ hɛzˈar usˈesˈɛdu tʃˈɪlupˈendʒ");
        assert_eq!(cardinal("2500000"), "dˈʊ mɪljˈon upˈendʒsˈɛd hɛzˈar");
        assert_eq!(cardinal("1000001"), "mɪljˈon u jˈɛk");
    }

    #[test]
    fn leading_zeros_are_spoken() {
        assert_eq!(cardinal("00"), "sˈɪfr sˈɪfr");
        assert_eq!(cardinal("007"), "sˈɪfrsˈɪfr hˈɛft");
        assert_eq!(cardinal("0007"), "sˈɪfrsˈɪfrsˈɪfrhˈɛft");
    }

    #[test]
    fn decimals_speak_fraction_digits_one_by_one() {
        assert_eq!(decimal("3", "14"), "sˈe xal jˈɛk dʒˈar");
    }
}
