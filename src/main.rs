use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod linter;

use linter::{RuleConfig, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (paths, config, format) = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("phonelint: {}", e);
            return ExitCode::from(2);
        }
    };

    let mut found_error = false;
    let mut had_io_error = false;

    if paths.is_empty() {
        scan_stdin(&config, format, &mut found_error, &mut had_io_error);
    } else {
        for path in &paths {
            if path == "-" {
                scan_stdin(&config, format, &mut found_error, &mut had_io_error);
                continue;
            }

            let files = match expand_path(Path::new(path)) {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("phonelint: cannot open {}: {}", path, e);
                    had_io_error = true;
                    continue;
                }
            };

            for file in files {
                let label = file.display().to_string();
                match File::open(&file) {
                    Ok(f) => match run(&label, BufReader::new(f), &config, format) {
                        Ok(err) => found_error |= err,
                        Err(e) => {
                            eprintln!("phonelint: read error on {}: {}", label, e);
                            had_io_error = true;
                        }
                    },
                    Err(e) => {
                        eprintln!("phonelint: cannot open {}: {}", label, e);
                        had_io_error = true;
                    }
                }
            }
        }
    }

    if had_io_error {
        ExitCode::from(2)
    } else if found_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn scan_stdin(config: &RuleConfig, format: OutputFormat, found_error: &mut bool, had_io_error: &mut bool) {
    match run("<stdin>", BufReader::new(io::stdin()), config, format) {
        Ok(err) => *found_error |= err,
        Err(e) => {
            eprintln!("phonelint: read error: {}", e);
            *had_io_error = true;
        }
    }
}

// Expands a command-line path into the files it names: the path itself if
// it's a file, or every file found by walking it if it's a directory. This
// is what lets a single directory argument stand in for listing its files
// by hand.
fn expand_path(path: &Path) -> io::Result<Vec<PathBuf>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut files = Vec::new();
    collect_dir(path, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_dir(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_dir(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

// Parses everything but the input paths into a RuleConfig. Kept separate
// from main so the two can be tested and reasoned about without touching
// real files or stdin.
fn parse_args(args: &[String]) -> Result<(Vec<String>, RuleConfig, OutputFormat), String> {
    let mut paths = Vec::new();
    let mut config = RuleConfig::default();
    let mut format = OutputFormat::Text;
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
            "--json" => {
                format = OutputFormat::Json;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option '{}'", other));
            }
            other => {
                paths.push(other.to_string());
            }
        }
    }

    Ok((paths, config, format))
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
fn run<R: BufRead>(
    label: &str,
    mut reader: R,
    config: &RuleConfig,
    format: OutputFormat,
) -> io::Result<bool> {
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
            match format {
                OutputFormat::Text => println!(
                    "{}:{}:{}: [{}][{}] {}",
                    label, finding.line, finding.column, finding.severity, finding.rule, finding.message
                ),
                OutputFormat::Json => println!(
                    "{{\"file\":\"{}\",\"line\":{},\"column\":{},\"severity\":\"{}\",\"rule\":\"{}\",\"message\":\"{}\"}}",
                    json_escape(label),
                    finding.line,
                    finding.column,
                    finding.severity,
                    finding.rule,
                    json_escape(&finding.message),
                ),
            }
        }
    }

    Ok(found_error)
}

// Findings are printed one at a time as they're found, so JSON output is
// newline-delimited (one object per line) rather than a single array - that
// way a CI job can start reading results before a huge file finishes
// scanning, and the printer never has to buffer every finding in memory.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_means_stdin_and_default_config() {
        let (paths, _config, format) = parse_args(&[]).unwrap();
        assert!(paths.is_empty());
        assert_eq!(format, OutputFormat::Text);
    }

    #[test]
    fn positional_arg_is_the_path() {
        let (paths, _config, _format) = parse_args(&["file.csv".to_string()]).unwrap();
        assert_eq!(paths, vec!["file.csv".to_string()]);
    }

    #[test]
    fn multiple_positional_args_are_all_kept() {
        let (paths, _config, _format) =
            parse_args(&["a.csv".to_string(), "b.csv".to_string()]).unwrap();
        assert_eq!(paths, vec!["a.csv".to_string(), "b.csv".to_string()]);
    }

    #[test]
    fn json_flag_switches_output_format() {
        let (paths, _config, format) =
            parse_args(&["--json".to_string(), "file.csv".to_string()]).unwrap();
        assert_eq!(paths, vec!["file.csv".to_string()]);
        assert_eq!(format, OutputFormat::Json);
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
    fn valid_disable_and_severity_flags_combine_with_a_path() {
        let (paths, _config, _format) = parse_args(&[
            "--disable".to_string(),
            "phone-mixed-separators".to_string(),
            "--severity".to_string(),
            "phone-digit-count=warning".to_string(),
            "file.csv".to_string(),
        ])
        .unwrap();
        assert_eq!(paths, vec!["file.csv".to_string()]);
    }

    #[test]
    fn json_escape_handles_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"a "quoted" \path"#), r#"a \"quoted\" \\path"#);
    }

    #[test]
    fn json_escape_handles_control_characters() {
        assert_eq!(json_escape("a\nb\tc"), "a\\nb\\tc");
        assert_eq!(json_escape("\u{7}"), "\\u0007");
    }

    #[test]
    fn json_escape_leaves_plain_text_unchanged() {
        assert_eq!(json_escape("555-123-4567"), "555-123-4567");
    }

    // Tests below touch the filesystem, under a name unique to the test so
    // parallel test threads never share a directory.
    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("phonelint_test_{}_{}", name, std::process::id()))
    }

    #[test]
    fn expand_path_returns_a_single_file_unchanged() {
        let dir = unique_temp_dir("single_file");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        fs::write(&file, "555-123-4567").unwrap();

        assert_eq!(expand_path(&file).unwrap(), vec![file.clone()]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_path_walks_a_directory_recursively() {
        let dir = unique_temp_dir("directory_walk");
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("a.txt"), "one").unwrap();
        fs::write(dir.join("nested").join("b.txt"), "two").unwrap();

        let mut result = expand_path(&dir).unwrap();
        result.sort();
        assert_eq!(
            result,
            vec![dir.join("a.txt"), dir.join("nested").join("b.txt")]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_path_on_missing_path_is_an_error() {
        let dir = unique_temp_dir("missing_path");
        assert!(expand_path(&dir).is_err());
    }
}
