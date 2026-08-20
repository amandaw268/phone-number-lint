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

    check_digit_count(line_number, column, trimmed, digit_count, findings);
    check_separator_consistency(line_number, column, trimmed, findings);
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
