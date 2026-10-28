use crate::spell::{cardinal_words, with_suffix};

const MONTHS: [&str; 12] = [
    "Çile", "Sibat", "Adar", "Nîsan", "Gulan", "Hezîran", "Tîrmeh", "Tebax", "Îlon", "Çirî",
    "Mijdar", "Kanûn",
];

fn spelled(value: usize) -> Vec<String> {
    cardinal_words(value)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn twelve_hour(hour: usize) -> usize {
    match hour % 12 {
        0 => 12,
        h => h,
    }
}

fn minutes_past(hour: usize, minute: usize) -> Vec<String> {
    let words = |parts: &[&str]| parts.iter().map(|w| (*w).to_string()).collect::<Vec<_>>();
    let hour_words = |h| spelled(twelve_hour(h));
    match minute {
        0 => hour_words(hour),
        15 => [hour_words(hour), words(&["û", "çaryek"])].concat(),
        30 => [hour_words(hour), words(&["û", "nîv"])].concat(),
        45 => [hour_words(hour + 1), words(&["kêm", "çaryek"])].concat(),
        m => [
            hour_words(hour),
            words(&["û"]),
            spelled(m),
            words(&["deqîqe"]),
        ]
        .concat(),
    }
}

pub(crate) fn time_words(hour: usize, minute: usize) -> Vec<String> {
    std::iter::once("saet".to_string())
        .chain(minutes_past(hour, minute))
        .collect()
}

fn ezafe(mut words: Vec<String>) -> Vec<String> {
    if let Some(last) = words.pop() {
        words.push(with_suffix(&last, "ê", "yê"));
    }
    words
}

pub(crate) fn date_words(day: usize, month: usize, year: usize) -> Option<Vec<String>> {
    let month_name = MONTHS.get(month.checked_sub(1)?)?;
    Some(
        ezafe(spelled(day))
            .into_iter()
            .chain(ezafe(vec![(*month_name).to_string()]))
            .chain(cardinal_words(year)?.into_iter().map(str::to_string))
            .collect(),
    )
}

fn fraction_word(fraction: &str) -> Option<&'static str> {
    match fraction.trim_end_matches('0') {
        "5" => Some("nîv"),
        "25" => Some("çaryek"),
        _ => None,
    }
}

pub(crate) fn fraction_words(integer: &str, fraction: &str) -> Option<Vec<String>> {
    let word = fraction_word(fraction)?.to_string();
    let whole: usize = integer.parse().ok()?;
    (whole.to_string() == integer).then_some(())?;
    cardinal_words(whole)?;
    Some(match whole {
        0 => vec![word],
        _ => [spelled(whole), vec!["û".to_string(), word]].concat(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(words: Vec<String>) -> String {
        words.join(" ")
    }

    #[test]
    fn whole_hours_use_the_twelve_hour_clock() {
        assert_eq!(said(time_words(1, 0)), "saet yek");
        assert_eq!(said(time_words(13, 0)), "saet yek");
        assert_eq!(said(time_words(0, 0)), "saet diwanzde");
    }

    #[test]
    fn quarters_and_halves_use_their_words() {
        assert_eq!(said(time_words(3, 15)), "saet sê û çaryek");
        assert_eq!(said(time_words(9, 30)), "saet neh û nîv");
        assert_eq!(said(time_words(12, 45)), "saet yek kêm çaryek");
        assert_eq!(said(time_words(23, 45)), "saet diwanzde kêm çaryek");
    }

    #[test]
    fn other_minutes_are_added_to_the_hour() {
        assert_eq!(said(time_words(10, 10)), "saet deh û deh deqîqe");
        assert_eq!(said(time_words(4, 40)), "saet çar û çil deqîqe");
        assert_eq!(said(time_words(8, 41)), "saet heşt û çil û yek deqîqe");
    }

    #[test]
    fn dates_put_ezafe_on_the_day_and_the_month() {
        assert_eq!(
            said(date_words(28, 10, 2026).unwrap_or_default()),
            "bîst û heştê Çirîyê du hezar û bîst û şeş"
        );
        assert_eq!(
            said(date_words(2, 3, 2026).unwrap_or_default()),
            "duyê Adarê du hezar û bîst û şeş"
        );
    }

    #[test]
    fn halves_and_quarters_read_as_u_niv_and_u_caryek() {
        let said_for =
            |integer, fraction| said(fraction_words(integer, fraction).unwrap_or_default());
        assert_eq!(said_for("3", "5"), "sê û nîv");
        assert_eq!(said_for("21", "50"), "bîst û yek û nîv");
        assert_eq!(said_for("0", "5"), "nîv");
        assert_eq!(said_for("2", "25"), "du û çaryek");
        assert_eq!(said_for("0", "25"), "çaryek");
        assert!(fraction_words("3", "55").is_none());
        assert!(fraction_words("3", "05").is_none());
        assert!(fraction_words("007", "5").is_none());
    }

    #[test]
    fn invalid_months_have_no_date_words() {
        assert!(date_words(1, 13, 2026).is_none());
        assert!(date_words(1, 0, 2026).is_none());
    }
}
