use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::process::ExitCode;

mod linter;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).map(String::as_str);

    let result = match path {
        Some(p) if p != "-" => match File::open(p) {
            Ok(f) => run(p, BufReader::new(f)),
            Err(e) => {
                eprintln!("phonelint: cannot open {}: {}", p, e);
                return ExitCode::from(2);
            }
        },
        _ => run("<stdin>", BufReader::new(io::stdin())),
    };

    match result {
        Ok(found_any) => {
            if found_any {
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

// Reads one line at a time into a buffer that gets cleared and reused, so a
// ten-line file and a ten-gigabyte file cost the same amount of memory.
fn run<R: BufRead>(label: &str, mut reader: R) -> io::Result<bool> {
    let mut line = String::new();
    let mut line_number = 0usize;
    let mut found_any = false;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;
        let text = line.trim_end_matches(['\n', '\r']);

        for finding in linter::scan_line(line_number, text) {
            found_any = true;
            println!(
                "{}:{}:{}: [{}] {}",
                label, finding.line, finding.column, finding.rule, finding.message
            );
        }
    }

    Ok(found_any)
}
