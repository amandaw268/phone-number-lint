// A phone number rarely appears alone in a file: it's inside a CSV row, a log
// line, a support ticket. So instead of requiring the whole line to be a
// number, we scan for runs of digits and phone-shaped punctuation and only
// judge the parts that look like a number in the first place.

const SEPARATOR_CHARS: [char; 6] = ['+', '-', '.', ' ', '(', ')'];

// Below this many digits a run is more likely a page number or a list index
// than a phone number; above it, more likely an account or tracking id.
const MIN_PHONE_DIGITS: usize = 7;
const MAX_PHONE_DIGITS: usize = 15;

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub rule: &'static str,
    pub message: String,
}

fn is_candidate_char(c: char) -> bool {
    c.is_ascii_digit() || SEPARATOR_CHARS.contains(&c)
}

// Trims the prose punctuation that commonly borders a number in running
// text (sentence dashes, trailing periods, surrounding spaces) without
// trimming '+', '(' or ')', which can be meaningful parts of the number itself.
fn is_prose_punct(c: char) -> bool {
    c == ' ' || c == '.' || c == '-'
}

pub fn scan_line(line_number: usize, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut run_start: Option<usize> = None;

    for (idx, c) in text.char_indices() {
        if is_candidate_char(c) {
            if run_start.is_none() {
                run_start = Some(idx);
            }
        } else if let Some(start) = run_start.take() {
            evaluate_run(line_number, text, start, idx, &mut findings);
        }
    }
    if let Some(start) = run_start {
        evaluate_run(line_number, text, start, text.len(), &mut findings);
    }

    findings
}

fn evaluate_run(line_number: usize, text: &str, start: usize, end: usize, findings: &mut Vec<Finding>) {
    let raw = &text[start..end];
    let trimmed = raw.trim_matches(is_prose_punct);
    if trimmed.is_empty() {
        return;
    }

    // All separator characters are single-byte ASCII, so byte offsets and
    // char counts line up and this arithmetic is safe.
    let leading_trimmed = raw.len() - raw.trim_start_matches(is_prose_punct).len();
    let column = start + leading_trimmed + 1; // columns are 1-based

    let digit_count = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count < MIN_PHONE_DIGITS || digit_count > MAX_PHONE_DIGITS {
        return;
    }

    if looks_like_date(trimmed) || looks_like_decimal_number(trimmed) {
        return;
    }

    check_digit_count(line_number, column, trimmed, digit_count, findings);
    check_separator_consistency(line_number, column, trimmed, findings);
}

// A plain decimal number - one dot, digits on both sides, nothing else in
// the run - reads as a number, not a phone number. Real phone numbers that
// use '.' as a separator do so more than once (555.123.4567), so a single
// dot is the distinguishing signal here.
fn looks_like_decimal_number(trimmed: &str) -> bool {
    if trimmed.starts_with('+') || trimmed.matches('.').count() != 1 {
        return false;
    }
    if trimmed.chars().any(|c| c == '-' || c == ' ' || c == '(' || c == ')') {
        return false;
    }

    let mut parts = trimmed.split('.');
    let before = parts.next().unwrap_or("");
    let after = parts.next().unwrap_or("");
    !before.is_empty()
        && !after.is_empty()
        && before.chars().all(|c| c.is_ascii_digit())
        && after.chars().all(|c| c.is_ascii_digit())
}

// Matches an ISO-ish date (2024-01-01) or day-first date (01-01-2024): three
// dash-separated all-digit groups, one of them four digits long for the
// year, the other two short enough to be a month and a day, and the values
// themselves in range. Phone numbers grouped 4-3-2 or similar don't pass
// the range check and fall through to the normal rules.
fn looks_like_date(trimmed: &str) -> bool {
    if trimmed
        .chars()
        .any(|c| c == '.' || c == ' ' || c == '+' || c == '(' || c == ')')
    {
        return false;
    }

    let groups: Vec<&str> = trimmed.split('-').collect();
    if groups.len() != 3 || groups.iter().any(|g| g.is_empty() || !g.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }

    let lens: Vec<usize> = groups.iter().map(|g| g.len()).collect();
    let (year_idx, month_idx, day_idx) = if lens[0] == 4 && lens[1] <= 2 && lens[2] <= 2 {
        (0, 1, 2)
    } else if lens[2] == 4 && lens[0] <= 2 && lens[1] <= 2 {
        (2, 0, 1)
    } else {
        return false;
    };

    let year: u32 = groups[year_idx].parse().unwrap_or(0);
    let month: u32 = groups[month_idx].parse().unwrap_or(0);
    let day: u32 = groups[day_idx].parse().unwrap_or(0);
    (1900..=2100).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn check_digit_count(
    line_number: usize,
    column: usize,
    trimmed: &str,
    digit_count: usize,
    findings: &mut Vec<Finding>,
) {
    let has_country_prefix = trimmed.starts_with('+');
    let is_plausible = if has_country_prefix {
        (8..=MAX_PHONE_DIGITS).contains(&digit_count)
    } else {
        matches!(digit_count, 7 | 10 | 11)
    };

    if !is_plausible {
        findings.push(Finding {
            line: line_number,
            column,
            rule: "phone-digit-count",
            message: format!(
                "'{}' has {} digits, which is not a common phone number length",
                trimmed, digit_count
            ),
        });
    }
}

fn check_separator_consistency(
    line_number: usize,
    column: usize,
    trimmed: &str,
    findings: &mut Vec<Finding>,
) {
    let mut seps_seen = Vec::new();
    for c in trimmed.chars() {
        if (c == '-' || c == '.' || c == ' ') && !seps_seen.contains(&c) {
            seps_seen.push(c);
        }
    }

    if seps_seen.len() > 1 {
        findings.push(Finding {
            line: line_number,
            column,
            rule: "phone-mixed-separators",
            message: format!(
                "'{}' mixes separator styles {:?} within one number",
                trimmed, seps_seen
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(line: &str) -> Vec<&'static str> {
        scan_line(1, line).iter().map(|f| f.rule).collect()
    }

    #[test]
    fn clean_us_number_with_dashes_is_not_flagged() {
        assert_eq!(rules("call 555-123-4567 now"), Vec::<&str>::new());
    }

    #[test]
    fn clean_ten_digit_number_no_separators_is_not_flagged() {
        assert_eq!(rules("5551234567"), Vec::<&str>::new());
    }

    #[test]
    fn clean_international_number_is_not_flagged() {
        assert_eq!(rules("+44 20 7946 0958"), Vec::<&str>::new());
    }

    #[test]
    fn mixed_dash_and_dot_is_flagged() {
        assert_eq!(rules("555-123.4567"), vec!["phone-mixed-separators"]);
    }

    #[test]
    fn mixed_space_and_dash_is_flagged() {
        assert_eq!(rules("(555) 123-4567"), vec!["phone-mixed-separators"]);
    }

    #[test]
    fn nine_digit_run_is_flagged_for_digit_count() {
        assert_eq!(rules("1234-567-89"), vec!["phone-digit-count"]);
    }

    #[test]
    fn run_below_minimum_digits_is_ignored() {
        assert_eq!(rules("call 12-3456 today"), Vec::<&str>::new());
    }

    #[test]
    fn run_above_maximum_digits_is_ignored() {
        assert_eq!(rules("1234567890123456"), Vec::<&str>::new());
    }

    #[test]
    fn plus_prefixed_number_needs_at_least_eight_digits() {
        assert_eq!(rules("+1 555 123"), vec!["phone-digit-count"]);
    }

    #[test]
    fn column_accounts_for_leading_prose_and_is_one_based() {
        let findings = scan_line(1, "phone: 1234-567-89.");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].column, 8);
        assert_eq!(findings[0].line, 1);
    }

    #[test]
    fn multiple_candidates_on_one_line_are_each_evaluated() {
        assert_eq!(
            rules("555-123-4567 and 1234-567-89 and 555.123.4567"),
            vec!["phone-digit-count"]
        );
    }

    #[test]
    fn decimal_number_is_not_flagged() {
        assert_eq!(rules("pi is 3.1415926"), Vec::<&str>::new());
    }

    #[test]
    fn decimal_number_with_few_digits_after_point_is_not_flagged() {
        assert_eq!(rules("total: 12345678.9"), Vec::<&str>::new());
    }

    #[test]
    fn iso_date_is_not_flagged() {
        assert_eq!(rules("created on 2024-01-01"), Vec::<&str>::new());
    }

    #[test]
    fn day_first_date_is_not_flagged() {
        assert_eq!(rules("due 01-01-2024"), Vec::<&str>::new());
    }

    #[test]
    fn dash_grouped_run_with_out_of_range_month_is_still_evaluated() {
        // 4-2-2 grouping like a date, but month 19 isn't valid, so this
        // should fall through to the normal digit-count check.
        assert_eq!(rules("2024-19-99"), vec!["phone-digit-count"]);
    }

    #[test]
    fn empty_line_has_no_findings() {
        assert_eq!(rules(""), Vec::<&str>::new());
    }
}
