use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::process::ExitCode;

mod linter;

use linter::{RuleConfig, Severity};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (path, config) = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("phonelint: {}", e);
            return ExitCode::from(2);
        }
    };

    let result = match path.as_deref() {
        Some(p) if p != "-" => match File::open(p) {
            Ok(f) => run(p, BufReader::new(f), &config),
            Err(e) => {
                eprintln!("phonelint: cannot open {}: {}", p, e);
                return ExitCode::from(2);
            }
        },
        _ => run("<stdin>", BufReader::new(io::stdin()), &config),
    };

    match result {
        Ok(found_error) => {
            if found_error {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("phonelint: read error: {}", e);
            ExitCode::from(2)
        }
    }
}

// Parses everything but the input path into a RuleConfig. Kept separate from
// main so the two can be tested and reasoned about without touching real
// files or stdin.
fn parse_args(args: &[String]) -> Result<(Option<String>, RuleConfig), String> {
    let mut path = None;
    let mut config = RuleConfig::default();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--disable" => {
                let rule = iter
                    .next()
                    .ok_or_else(|| "--disable requires a rule name".to_string())?;
                config.disable(known_rule(rule)?);
            }
            "--severity" => {
                let spec = iter
                    .next()
                    .ok_or_else(|| "--severity requires RULE=LEVEL".to_string())?;
                let (rule, level) = spec
                    .split_once('=')
                    .ok_or_else(|| format!("--severity value '{}' is not RULE=LEVEL", spec))?;
                let severity = Severity::parse(level)
                    .ok_or_else(|| format!("unknown severity '{}', expected warning or error", level))?;
                config.set_severity(known_rule(rule)?, severity);
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option '{}'", other));
            }
            other => {
                if path.is_some() {
                    return Err("only one input path is supported".to_string());
                }
                path = Some(other.to_string());
            }
        }
    }

    Ok((path, config))
}

fn known_rule(name: &str) -> Result<&'static str, String> {
    linter::RULE_NAMES
        .iter()
        .find(|&&r| r == name)
        .copied()
        .ok_or_else(|| format!("unknown rule '{}'", name))
}

// Reads one line at a time into a buffer that gets cleared and reused, so a
// ten-line file and a ten-gigabyte file cost the same amount of memory.
// Returns whether any error-severity finding was reported; warnings are
// still printed but don't affect the exit code.
fn run<R: BufRead>(label: &str, mut reader: R, config: &RuleConfig) -> io::Result<bool> {
    let mut line = String::new();
    let mut line_number = 0usize;
    let mut found_error = false;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;
        let text = line.trim_end_matches(['\n', '\r']);

        for finding in linter::scan_line(line_number, text, config) {
            if finding.severity == Severity::Error {
                found_error = true;
            }
            println!(
                "{}:{}:{}: [{}][{}] {}",
                label, finding.line, finding.column, finding.severity, finding.rule, finding.message
            );
        }
    }

    Ok(found_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_means_stdin_and_default_config() {
        let (path, _config) = parse_args(&[]).unwrap();
        assert_eq!(path, None);
    }

    #[test]
    fn positional_arg_is_the_path() {
        let (path, _config) = parse_args(&["file.csv".to_string()]).unwrap();
        assert_eq!(path.as_deref(), Some("file.csv"));
    }

    #[test]
    fn disable_flag_rejects_unknown_rule() {
        let err = parse_args(&["--disable".to_string(), "not-a-rule".to_string()]).unwrap_err();
        assert!(err.contains("unknown rule"));
    }

    #[test]
    fn severity_flag_requires_equals_form() {
        let err = parse_args(&["--severity".to_string(), "phone-digit-count".to_string()]).unwrap_err();
        assert!(err.contains("RULE=LEVEL"));
    }

    #[test]
    fn severity_flag_rejects_unknown_level() {
        let err = parse_args(&[
            "--severity".to_string(),
            "phone-digit-count=fatal".to_string(),
        ])
        .unwrap_err();
        assert!(err.contains("unknown severity"));
    }

    #[test]
    fn two_positional_paths_is_an_error() {
        let err = parse_args(&["a.csv".to_string(), "b.csv".to_string()]).unwrap_err();
        assert!(err.contains("only one input path"));
    }

    #[test]
    fn valid_disable_and_severity_flags_combine_with_a_path() {
        let (path, _config) = parse_args(&[
            "--disable".to_string(),
            "phone-mixed-separators".to_string(),
            "--severity".to_string(),
            "phone-digit-count=warning".to_string(),
            "file.csv".to_string(),
        ])
        .unwrap();
        assert_eq!(path.as_deref(), Some("file.csv"));
    }
}
