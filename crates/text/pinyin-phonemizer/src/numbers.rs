const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

fn digit_to_word(d: u32) -> char {
    DIGITS[d as usize]
}

const MAX_CARDINAL_DIGITS: usize = 8;
const YEAR_DIGITS: usize = 4;

fn read_digit_by_digit(digits: &str) -> String {
    digits
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(digit_to_word)
        .collect()
}

fn format_section(n: u32) -> String {
    let places = [
        (n / 1000, Some('千')),
        (n / 100 % 10, Some('百')),
        (n / 10 % 10, Some('十')),
        (n % 10, None),
    ];
    let mut out = String::new();
    let mut pending_zero = false;
    for (digit, unit) in places {
        if digit == 0 {
            pending_zero = !out.is_empty();
            continue;
        }
        if pending_zero {
            out.push('零');
            pending_zero = false;
        }
        out.push(digit_to_word(digit));
        out.extend(unit);
    }
    out
}

fn format_cardinal(n: u32) -> String {
    let high = n / 10_000;
    let low = n % 10_000;
    let mut out = String::new();
    if high > 0 {
        out.push_str(&format_section(high));
        out.push('万');
        if low > 0 && low < 1000 {
            out.push('零');
        }
    }
    if low > 0 || high == 0 {
        out.push_str(&format_section(low));
    }
    match out.strip_prefix("一十") {
        Some(rest) => format!("十{rest}"),
        None if out.is_empty() => digit_to_word(0).to_string(),
        None => out,
    }
}

fn format_integer(digits: &str) -> String {
    let has_leading_zero = digits.len() > 1 && digits.starts_with('0');
    if has_leading_zero || digits.len() > MAX_CARDINAL_DIGITS {
        return read_digit_by_digit(digits);
    }
    match digits.parse::<u32>() {
        Ok(n) => format_cardinal(n),
        Err(_) => read_digit_by_digit(digits),
    }
}

fn format_fraction_digits(digits: &str) -> String {
    read_digit_by_digit(digits)
}

pub fn normalize_numbers(text: &str) -> String {
    let text = normalize_temperatures(text);
    let text = normalize_percentages(&text);
    normalize_plain_digits(&text)
}

fn normalize_temperatures(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let negative = chars[i] == '-' || chars[i] == '\u{2212}';
        let digit_start = if negative { i + 1 } else { i };
        let mut integer_end = digit_start;
        while integer_end < chars.len() && chars[integer_end].is_ascii_digit() {
            integer_end += 1;
        }
        if integer_end == digit_start {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut fraction_end = integer_end;
        if chars.get(integer_end) == Some(&'.') {
            fraction_end = integer_end + 1;
            while fraction_end < chars.len() && chars[fraction_end].is_ascii_digit() {
                fraction_end += 1;
            }
        }
        let value_end = if fraction_end > integer_end + 1 {
            fraction_end
        } else {
            integer_end
        };
        let mut k = value_end;
        while k < chars.len() && chars[k] == ' ' {
            k += 1;
        }
        let celsius_end = if chars.get(k) == Some(&'\u{2103}') {
            Some(k + 1)
        } else if chars.get(k) == Some(&'°') && chars.get(k + 1) == Some(&'C') {
            Some(k + 2)
        } else {
            None
        };
        let Some(end) = celsius_end else {
            out.extend(&chars[i..value_end]);
            i = value_end;
            continue;
        };
        let integer_digits: String = chars[digit_start..integer_end].iter().collect();
        let mut words = format_integer(&integer_digits);
        if value_end > integer_end {
            let fractional_digits: String = chars[integer_end + 1..value_end].iter().collect();
            words = format!("{words}点{}", format_fraction_digits(&fractional_digits));
        }
        let prefix = if negative { "零下" } else { "" };
        out.push_str(&format!("{prefix}{words}度"));
        i = end;
    }
    out
}

fn normalize_percentages(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let sign_start = i;
        let negative = chars[i] == '-' || chars[i] == '\u{2212}';
        let digit_start = if negative { i + 1 } else { i };

        let mut integer_end = digit_start;
        while integer_end < chars.len() && chars[integer_end].is_ascii_digit() {
            integer_end += 1;
        }
        let has_integer_digits = integer_end > digit_start;

        let mut fraction_end = integer_end;
        if has_integer_digits && integer_end < chars.len() && chars[integer_end] == '.' {
            fraction_end = integer_end + 1;
            while fraction_end < chars.len() && chars[fraction_end].is_ascii_digit() {
                fraction_end += 1;
            }
        }
        let has_fraction_digits = fraction_end > integer_end + 1;
        let value_end = if has_fraction_digits {
            fraction_end
        } else {
            integer_end
        };

        if has_integer_digits
            && value_end < chars.len()
            && (chars[value_end] == '%' || chars[value_end] == '\u{FF05}')
        {
            let integer_digits: String = chars[digit_start..integer_end].iter().collect();
            let mut words = format_integer(&integer_digits);
            if has_fraction_digits {
                let fractional_digits: String =
                    chars[integer_end + 1..fraction_end].iter().collect();
                words = format!("{words}点{}", format_fraction_digits(&fractional_digits));
            }
            if negative {
                out.push_str(&format!("负百分之{words}"));
            } else {
                out.push_str(&format!("百分之{words}"));
            }
            i = value_end + 1;
            continue;
        }
        out.push(chars[sign_start]);
        i += 1;
    }
    out
}

fn normalize_plain_digits(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let end = chars[i..]
            .iter()
            .position(|c| !c.is_ascii_digit())
            .map_or(chars.len(), |offset| i + offset);
        let run: String = chars[i..end].iter().collect();
        let is_year = run.len() == YEAR_DIGITS && chars.get(end) == Some(&'年');
        let is_fraction = i >= 2 && chars[i - 1] == '.' && chars[i - 2].is_ascii_digit();
        if is_fraction {
            out.pop();
            out.push('点');
            out.push_str(&read_digit_by_digit(&run));
        } else if is_year {
            out.push_str(&read_digit_by_digit(&run));
        } else {
            out.push_str(&format_integer(&run));
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalize_numbers_spells_out_single_digits() {
        assert_eq!(super::normalize_numbers("我有3个"), "我有三个");
    }

    #[test]
    fn normalize_numbers_spells_out_two_digit_numbers() {
        assert_eq!(super::normalize_numbers("23"), "二十三");
    }

    #[test]
    fn normalize_numbers_handles_a_multiple_of_ten_without_a_trailing_zero_digit() {
        assert_eq!(super::normalize_numbers("20"), "二十");
    }

    #[test]
    fn normalize_numbers_expands_a_percentage_with_bai_fen_zhi_prefix() {
        assert_eq!(super::normalize_numbers("77%"), "百分之七十七");
        assert_eq!(super::normalize_numbers("77％"), "百分之七十七");
    }

    #[test]
    fn normalize_numbers_expands_a_decimal_percentage_reading_the_fraction_digit_by_digit() {
        assert_eq!(super::normalize_numbers("98.76%"), "百分之九十八点七六");
    }

    #[test]
    fn normalize_numbers_expands_a_negative_percentage_with_a_fu_prefix() {
        assert_eq!(super::normalize_numbers("-7%"), "负百分之七");
    }

    #[test]
    fn normalize_numbers_expands_a_negative_decimal_percentage() {
        assert_eq!(super::normalize_numbers("-3.5%"), "负百分之三点五");
    }

    #[test]
    fn format_integer_falls_back_to_digit_by_digit_when_the_value_overflows_u64() {
        assert_eq!(
            super::format_integer("18446744073709551616"),
            "一八四四六七四四零七三七零九五五一六一六"
        );
    }

    #[test]
    fn normalize_numbers_expands_a_celsius_temperature() {
        assert_eq!(super::normalize_numbers("7°C"), "七度");
        assert_eq!(super::normalize_numbers("7℃"), "七度");
    }

    #[test]
    fn normalize_numbers_uses_ling_xia_for_a_negative_temperature() {
        assert_eq!(super::normalize_numbers("-7°C"), "零下七度");
    }

    #[test]
    fn normalize_numbers_expands_a_decimal_temperature() {
        assert_eq!(super::normalize_numbers("1.5℃"), "一点五度");
        assert_eq!(super::normalize_numbers("-36.6°C"), "零下三十六点六度");
    }

    #[test]
    fn normalize_temperatures_keeps_a_decimal_without_a_unit_intact() {
        assert_eq!(super::normalize_temperatures("3.14 and -2"), "3.14 and -2");
    }

    #[test]
    fn normalize_numbers_reads_a_bare_decimal_fraction_digit_by_digit() {
        assert_eq!(super::normalize_numbers("3.14"), "三点一四");
    }

    #[test]
    fn normalize_numbers_reads_hundreds_as_cardinals() {
        assert_eq!(super::normalize_numbers("150人"), "一百五十人");
        assert_eq!(super::normalize_numbers("100"), "一百");
        assert_eq!(super::normalize_numbers("101"), "一百零一");
        assert_eq!(super::normalize_numbers("110"), "一百一十");
    }

    #[test]
    fn normalize_numbers_reads_thousands_and_ten_thousands_as_cardinals() {
        assert_eq!(super::normalize_numbers("1001"), "一千零一");
        assert_eq!(super::normalize_numbers("1010"), "一千零一十");
        assert_eq!(super::normalize_numbers("2500"), "二千五百");
        assert_eq!(super::normalize_numbers("10000"), "一万");
        assert_eq!(super::normalize_numbers("10050"), "一万零五十");
        assert_eq!(super::normalize_numbers("123456"), "十二万三千四百五十六");
        assert_eq!(super::normalize_numbers("100000"), "十万");
    }

    #[test]
    fn normalize_numbers_keeps_leading_zeros_as_digits() {
        assert_eq!(super::normalize_numbers("007"), "零零七");
        assert_eq!(super::normalize_numbers("05"), "零五");
        assert_eq!(super::normalize_numbers("0"), "零");
    }

    #[test]
    fn normalize_numbers_reads_long_digit_runs_one_digit_at_a_time() {
        assert_eq!(
            super::normalize_numbers("13800138000"),
            "一三八零零一三八零零零"
        );
    }

    #[test]
    fn normalize_numbers_reads_a_four_digit_year_digit_by_digit() {
        assert_eq!(super::normalize_numbers("2024年"), "二零二四年");
        assert_eq!(super::normalize_numbers("10年"), "十年");
    }

    #[test]
    fn normalize_numbers_expands_a_hundreds_percentage() {
        assert_eq!(super::normalize_numbers("150%"), "百分之一百五十");
    }

    #[test]
    fn normalize_numbers_leaves_non_numeric_text_unchanged() {
        assert_eq!(super::normalize_numbers("你好"), "你好");
    }
}
