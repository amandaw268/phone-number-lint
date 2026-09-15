# phonelint

Every export from a CRM, a call log, or a support ticket queue ends up with
phone numbers written five different ways in the same file: `555-123-4567`,
`555.123.4567`, `(555) 123 4567`, `5551234567`. Nothing crashes because of
this, but it makes the data useless for dedup, search, or handing to a
formatter that expects one convention. phonelint scans text and points out
the numbers that don't look right, with a line and column so you can go fix
them.

## Usage

```
phonelint customers.csv
```

Pass more than one path, mix files and directories, or leave it pointed at
a directory and phonelint will scan every file it finds underneath it:

```
phonelint customers.csv tickets.csv
phonelint ./exports
```

```
cat support_tickets.log | phonelint
```

Sample output:

```
customers.csv:14:23: [error][phone-mixed-separators] '555-123.4567' mixes separator styles ['-', '.'] within one number
customers.csv:31:9: [error][phone-digit-count] '555-12-34' has 6 digits, which is not a common phone number length
```

Exit code is `0` if nothing at `error` severity was flagged, `1` if it was,
`2` on a read error (e.g. a file doesn't exist). Findings at `warning`
severity are still printed but don't affect the exit code, so they can be
used for informational output in CI without failing the build. When
scanning several paths, a read error on one of them doesn't stop the
others - phonelint keeps going and reports exit code `2` at the end if any
path failed.

## Rules (v1)

- `phone-mixed-separators` — a single number uses more than one separator
  style, e.g. a dash and a dot in the same number.
- `phone-digit-count` — the digit count doesn't match a common phone number
  length (7 for a local number, 10 or 11 for US-style, 8-15 with a leading
  `+` for international).
- `phone-invalid-area-code` — a 10-digit or leading-`1` 11-digit number's
  area code doesn't fit the North American Numbering Plan: it starts with
  `0` or `1`, or it's a reserved `N11` service code (`211`, `411`, `911`,
  and so on). Numbers with a `+` country-code prefix are outside the NANP
  and skip this rule.
- `phone-invalid-exchange-code` — the same number's exchange code (the
  central office code, the second group of three digits) fails the same
  NANP shape check as the area code: it can't start with `0` or `1`, and
  it can't be a reserved `N11` code either. Same scope as the area-code
  rule - `+`-prefixed numbers skip it.

All rules run at `error` severity by default. Turn a rule off entirely, or
change its severity, from the command line:

```
phonelint --disable phone-digit-count customers.csv
phonelint --severity phone-mixed-separators=warning customers.csv
```

Both flags can be repeated and combined.

## JSON output

Pass `--json` to get newline-delimited JSON instead of the plain-text format,
one object per finding, printed as it's found rather than collected into an
array:

```
phonelint --json customers.csv
```

```
{"file":"customers.csv","line":14,"column":23,"severity":"error","rule":"phone-mixed-separators","message":"'555-123.4567' mixes separator styles ['-', '.'] within one number"}
{"file":"customers.csv","line":31,"column":9,"severity":"error","rule":"phone-digit-count","message":"'555-12-34' has 6 digits, which is not a common phone number length"}
```

Each line is a self-contained JSON object, so a CI job can parse results as
they arrive instead of waiting for the whole file to finish scanning.

## How it decides what's a phone number

There's no library of real number plans here, just a heuristic: scan for
runs of digits and phone-shaped punctuation (`+ - . ( ) `), trim the prose
punctuation off the edges, and if what's left has between 7 and 15 digits,
treat it as a candidate and run the rules on it. Two shapes are recognized
and excluded before the rules run: a plain decimal number (`3.1415926`, one
dot, digits on both sides) and a dash-grouped date (`2024-01-01` or
`01-01-2024`, a 4-digit year plus a valid month and day). Everything else
that falls in the digit-count range gets treated as a candidate - long IDs
can still occasionally get flagged, see Limitations below.

## Why streaming matters here

Log files and CSV exports are the natural input, and those can be large.
phonelint reads one line at a time into a reused buffer instead of loading
the file into a string first, so scanning a multi-gigabyte log costs about
the same memory as scanning a ten-line one.

## Limitations

- No real number-plan knowledge. Plain decimals and dash-grouped dates are
  filtered out (see above), but other digit runs that happen to fall in
  phone-number range - order numbers, tracking IDs, slash-separated dates -
  can still be misflagged.
- No per-extension rules yet - a phone-shaped run in a `.json` file is
  judged the same way as one in a `.csv` file.
- Area-code and exchange-code validation only cover the North American
  Numbering Plan; numbers with a `+` country-code prefix aren't checked
  against any real numbering plan at all.

## Building

Standard library only, no external crates:

```
cargo build --release
```

## License

MIT, see LICENSE.
