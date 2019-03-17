//! **tbuck** ("timeseries bucketing") by Drake Tetreault
//!
//! To the extent possible under law, the person who associated CC0 with
//! tbuck has waived all copyright and related or neighboring rights
//! to tbuck.
//!
//! You should have received a copy of the CC0 legalcode along with this
//! work.  If not, see <http://creativecommons.org/publicdomain/zero/1.0/>.

#![deny(clippy::all)]
#![deny(clippy::pedantic)]

use std::cmp::Ordering;
use std::io::{BufRead, BufReader, Read, Result as IoResult, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use chrono::format::strftime::StrftimeItems;
use chrono::format::{Fixed, Item, Numeric, Pad, Parsed};
use chrono::{DateTime, Duration, Timelike, Utc};
use clap::{App, Arg};
use hashbrown::HashMap;
use regex::Regex;

fn main() -> IoResult<()> {
    let args = parse_args();

    // Single line buffer to avoid allocating for each line.
    let mut line = String::with_capacity(4096);

    // Compile the regex only once.
    let regex = args.datetime_format.regex();

    // Initialize mode-based logic.
    let mut runner = Runner::from_mode(args.mode);

    // TODO: parallelize reading across inputs? Probably not super helpful.
    for input in &args.inputs {
        // open_bare_read does dynamic dispatch based on the type of input via a `&mut dyn Read` pointer.
        input.open_bare_read(|read| {
            let mut reader = BufReader::new(read);
            loop {
                // Always clear old data.
                line.clear();

                if reader.read_line(&mut line)? == 0 {
                    break;
                }

                // Find the match at the indicated match_index. Ignore lines without a match.
                let match_ = match regex.find_iter(&line).skip(args.match_index).nth(0) {
                    None => continue,
                    Some(m) => m,
                };

                // Convert the match into a DateTime<Utc>. Because the regex is more permissive than
                // the chrono library (for example, a value of '61' seconds will pass the regex but
                // not chrono's range checking), its possible the parsing may fail. This is more
                // indicative of a problem than a line not having a match, so alert the user with
                // a stderr message.
                let datetime = match args.datetime_format.try_parse(match_.as_str()) {
                    Ok(p) => p,
                    Err(err) => {
                        eprintln!("Failed to parse date/time match: {}", err);
                        continue;
                    }
                };

                // Increment bucket count.
                let bucket = args.granularity.bucketize(&datetime);
                runner.handle_bucket_entry(bucket, &args)?;
            }
            Ok(())
        })?;
    }

    runner.finish(&args)
}

// Defines CLI args. Will terminate program with an error message if args are invalid.
fn parse_args() -> Args {
    let app_matches = App::new("tbuck")
        .author(clap::crate_authors!())
        .version(clap::crate_version!())
        .about(clap::crate_description!())
        .arg(Arg::with_name("match-index")
            .short("m")
            .long("match-index")
            .takes_value(true)
            .value_name("MATCH_INDEX")
            .default_value("0")
            .help("0-based index of match to use if multiple matches are found")
            .validator(|value| {
                value.parse::<usize>()
                    .map(|_| ())
                    .map_err(|_| "Not a valid positive integer index".to_string())
            }))
        .arg(Arg::with_name("granularity")
            .short("g")
            .long("granularity")
            .takes_value(true)
            .value_name("GRANULARITY")
            .default_value("1m")
            .help("Bucket time granularity in seconds ('5s'), minutes ('1m'), or hours ('2h')")
            .validator(|value| {
                Granularity::parse(&value)
                    .map(|_| ())
                    .ok_or_else(|| "Not a valid granularity specifier".to_string())
            }))
        .arg(Arg::with_name("no-fill")
            .short("n")
            .long("no-fill")
            .help("Disable counts of 0 being emitted for buckets with no entries")
            .long_help("By default buckets which had no entries present will be displayed with a count of 0. If this flag is present then instead the bucket will not be printed at all."))
        .arg(Arg::with_name("stream")
            .short("s")
            .long("stream")
            .help("Enable stream mode")
            .long_help("Enable stream mode. Entries will be expected to arrive in monotonically increasing (or --decreasing) order, and bucket information will be printed live as soon as the bucket is known to be finished. By default the presence of any entry violating the monotonic order will cause an error, but this can be made --tolerant."))
        .arg(Arg::with_name("descending")
            .short("d")
            .long("descending")
            .help("Set expected stream order to descending, or prints buckets in descending order in normal mode")
            .long_help("By default stream mode expects entries to be in monotonically ascending order by date (earlier dates followed by later dates), which is the usual order of log files. If this flag is present then stream mode will instead expect entries in monotonically decreasing order by date (later dates followed by earlier dates). In normal mode, this flag will cause the buckets to be printed in descending order instead of the default ascending order."))
        .arg(Arg::with_name("tolerant")
            .short("t")
            .long("tolerant")
            .requires("stream")
            .help("Make stream mode silently discard non-monotonic entries instead of erroring")
            .long_help("By default when a non-monotonic entry is encountered in stream mode the program will terminate with an error. If this flag is present then non-monotonic entries will instead be silently discarded."))
        .arg(Arg::with_name("format")
            .required(true)
            .takes_value(true)
            .value_name("DATE_TIME_FORMAT")
            .help("Date/time parsing format; use --help for list of specifiers")
            .long_help(
"Date/time parsing format. Full date and time information must be present. The following specifiers are supported, taken from Rust's chrono crate:
Specifier   Example     Description
%Y          2001        The full proleptic Gregorian year, zero-padded to 4 digits.
%m          07          Month number (01--12), zero-padded to 2 digits.
%b          Jul         Abbreviated month name. Always 3 letters.
%B          July        Full month name. Also accepts corresponding abbreviation in parsing.
%d          08          Day number (01--31), zero-padded to 2 digits.
%F          2001-07-08  Year-month-day format (ISO 8601). Same to %Y-%m-%d.
%H          00          Hour number (00--23), zero-padded to 2 digits.
%I          12          Hour number in 12-hour clocks (01--12), zero-padded to 2 digits.
%M          34          Minute number (00--59), zero-padded to 2 digits.
%S          60          Second number (00--60), zero-padded to 2 digits.
%T          00:34:60    Hour-minute-second format. Same to %H:%M:%S.
%P          am          am or pm in 12-hour clocks.
%p          AM          AM or PM in 12-hour clocks.
%s          994518299   UNIX timestamp, the number of seconds since 1970-01-01 00:00 UTC.")
            .validator(|value| {
                DateTimeFormat::new(&value)
                    .ok_or_else(|| "Not a valid date/time format, use --help to list supported specifiers".to_string())
                    .and_then(|format| {
                        if format.has_enough_info() {
                            Ok(())
                        } else {
                            Err("Not enough information in the date/time format to construct a full date/time".to_string())
                        }
                    })
            }))
        .arg(Arg::with_name("inputs")
            .takes_value(true)
            .value_name("INPUT_FILE")
            .multiple(true)
            .help("Input files; or standard input if none provided"))
        .get_matches();

    let datetime_format = DateTimeFormat::new(
        app_matches
            .value_of("format")
            .expect("format is a required argument"),
    )
    .expect("validator should have rejected unsupported items");
    let match_index = app_matches.value_of("match-index").expect("match-index has default value")
        .parse::<usize>()
        .expect("validator should have rejected invalid values");
    let granularity = Granularity::parse(app_matches.value_of("granularity").expect("granularity has default value"))
        .expect("validator should have rejected invalid values");
    let inputs = app_matches.values_of_os("inputs").map_or_else(
        || vec![Input::Stdin {}],
        |vals| {
            vals.map(|val| Input::File(Path::new(val).to_path_buf()))
                .collect()
        },
    );
    let fill_empty_buckets = !app_matches.is_present("no-fill");
    let tolerant = app_matches.is_present("tolerant");
    let order = if app_matches.is_present("descending") { DateTimeOrder::Descending } else { DateTimeOrder::Ascending };
    let mode = if app_matches.is_present("stream") { Mode::Stream } else { Mode::Normal };

    Args {
        datetime_format,
        match_index,
        granularity,
        inputs,
        fill_empty_buckets,
        mode,
        order,
        tolerant,
    }
}

// Parsed CLI args.
#[derive(Debug)]
struct Args {
    datetime_format: DateTimeFormat,
    match_index: usize,
    granularity: Granularity,
    inputs: Vec<Input>,
    fill_empty_buckets: bool,
    mode: Mode,
    order: DateTimeOrder,
    tolerant: bool
}

#[derive(Debug, Copy, Clone)]
enum Mode {
    Normal,
    Stream
}

// Mode-based runner. Contains business logic for normal and streaming modes.
enum Runner {
    // Normal mode will put everything into buckets and print them all at the end.
    Normal {
        // Unordered buckets - will be ordered after all lines have been counted.
        buckets: HashMap<DateTime<Utc>, u64>
    },
    Stream {
        count: u64,
        bucket: Option<DateTime<Utc>>
    }
}

impl Runner {
    fn from_mode(mode: Mode) -> Self {
        match mode {
            Mode::Normal => Runner::Normal {
                buckets: HashMap::with_capacity(1024)
            },
            Mode::Stream => Runner::Stream {
                count: 0,
                bucket: None
            }
        }
    }

    fn handle_bucket_entry(&mut self, entry: DateTime<Utc>, args: &Args) -> IoResult<()> {
        match self {
            Runner::Normal { buckets } => {
                *buckets.entry(entry).or_insert(0) += 1;
                Ok(())
            },
            Runner::Stream { count, bucket } => {
                let current_bucket = match bucket {
                    Some(b) => b,
                    None => {
                        // If this is the first bucket, just record the entry and return.
                        *bucket = Some(entry);
                        *count = 1;
                        return Ok(());
                    }
                };
                // What to do next depends on both what ordering the user configured and what the actual relation between the
                // current bucket and new entry is.
                match (args.order, entry.cmp(current_bucket)) {
                    (_, Ordering::Equal) => {
                        // Same bucket. Just increment the count.
                        *count += 1;
                    },
                    (DateTimeOrder::Ascending, Ordering::Less) | (DateTimeOrder::Descending, Ordering::Greater) => {
                        // Non-monotonic according to configured ordering.
                        if !args.tolerant {
                            // TODO: better error propagation.
                            panic!("Non monotonic entry found");
                        }
                    },
                    (DateTimeOrder::Ascending, Ordering::Greater) | (DateTimeOrder::Descending, Ordering::Less) => {
                        // Monotonic. Print bucket(s) and advance to the next. We may be printing multiple buckets at
                        // once so lock stdout.
                        let stdout = std::io::stdout();
                        let mut stdout_lock = stdout.lock();
                        writeln!(stdout_lock, "{},{}", current_bucket, count)?;
                        if args.fill_empty_buckets {
                            let mut next_bucket = args.granularity.successor(current_bucket);
                            while next_bucket < entry {
                                writeln!(stdout_lock, "{},0", next_bucket)?;
                                next_bucket = args.granularity.successor(&next_bucket);
                            }
                        }
                        *count = 1;
                        *bucket = Some(entry)
                    },
                }
                Ok(())
            }
        }
    }

    fn finish(self, args: &Args) -> IoResult<()> {
        match self {
            Runner::Normal { buckets } => {
                // Sort buckets by time.
                let mut ordered_buckets: Vec<(DateTime<Utc>, u64)> = buckets.into_iter().collect();
                match args.order {
                    DateTimeOrder::Ascending => ordered_buckets.sort_unstable_by(|l, r| l.0.cmp(&r.0)),
                    DateTimeOrder::Descending => ordered_buckets.sort_unstable_by(|l, r| r.0.cmp(&l.0))
                };

                // Write output to stdout.
                let stdout = std::io::stdout();
                let mut stdout_lock = stdout.lock();
                let mut prev_bucket = chrono::MAX_DATE.and_hms(0, 0, 0);
                for (bucket, count) in &ordered_buckets {
                    // Unless --no-fill was specified, we need to emit 0s for buckets which don't exist.
                    if args.fill_empty_buckets {
                        while prev_bucket < *bucket {
                            writeln!(stdout_lock, "{},0", prev_bucket)?;
                            prev_bucket = args.granularity.successor(&prev_bucket);
                        }
                    }
                    writeln!(stdout_lock, "{},{}", bucket, count)?;
                    prev_bucket = args.granularity.successor(bucket);
                }
            },
            Runner::Stream { count, bucket } => {
                if let Some(bucket) = bucket {
                    // Don't bother locking stdout for a single write.
                    println!("{},{}", bucket, count);
                }
            }
        };
        Ok(())
    }
}

// The order that datetime entries are expected in stream mode OR the order that buckets
// will be printed in normal mode.
#[derive(Debug, Copy, Clone)]
enum DateTimeOrder {
    Ascending,
    Descending
}

// Where the program can take its input from.
#[derive(Debug)]
enum Input {
    Stdin,
    File(PathBuf),
}

impl Input {
    // Invoke a callback function that accepts a `&mut dyn Read` for dynamic dispatch based on the
    // type of input. This is mostly useful because it allows us to lock stdin for the entire
    // duration of the program.
    fn open_bare_read(&self, mut f: impl FnMut(&mut dyn Read) -> IoResult<()>) -> IoResult<()> {
        match self {
            Input::Stdin => {
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                f(&mut lock)
            }
            Input::File(path) => {
                let mut file = std::fs::File::open(path)?;
                f(&mut file)
            }
        }
    }
}

// Will be used both for finding timestamps within a line and parsing the timestamp into a datetime.
#[derive(Debug)]
struct DateTimeFormat {
    chrono_items: Vec<FormatItem>,
}

impl DateTimeFormat {
    // Parse the chrono format specifiers in a string into a DateTimeFormat. Returns Some() if all
    // the specifiers in the string are actually supported, or None if the user tried to use an
    // unsupported chrono specifier.
    fn new(format_string: &str) -> Option<Self> {
        let mut items_supported = true;
        let chrono_items: Vec<FormatItem> = StrftimeItems::new(format_string)
            .inspect(|item| {
                items_supported &= match item {
                    Item::Numeric(numeric, pad) => {
                        numeric_format_to_regex_fragment(numeric, *pad).is_some()
                    }
                    Item::Fixed(fixed) => fixed_format_to_regex_fragment(fixed).is_some(),
                    _ => true,
                }
            })
            .map(FormatItem::from_chrono)
            .collect();
        if items_supported {
            Some(Self { chrono_items })
        } else {
            None
        }
    }

    // Build the regex which can find occurrences of this format in a line.
    fn regex(&self) -> Regex {
        let mut expression = String::with_capacity(128);
        for item in &self.chrono_items {
            match item {
                FormatItem::Literal(string) | FormatItem::Space(string) => {
                    // Remember to escape special characters.
                    expression.push_str(&regex::escape(string));
                }
                FormatItem::Numeric(numeric, pad) => {
                    expression.push_str(
                        numeric_format_to_regex_fragment(numeric, *pad)
                            .expect("validator should have rejected unsupported items"),
                    );
                }
                FormatItem::Fixed(fixed) => {
                    expression.push_str(
                        fixed_format_to_regex_fragment(fixed)
                            .expect("validator should have rejected unsupported items"),
                    );
                }
            }
        }
        // Given that the only parts to the regex are A) user input that has been escaped and B) strings
        // that our code is responsible for, we expect the regex to be valid.
        Regex::new(&expression).expect("Regex unexpectedly invalid")
    }

    // Try to parse text that was matched by the regex into a DateTime<Utc>. This method's current
    // implementation calls Parsed::to_datetime_with_timezone, which has the major implication that
    // full date/time information must be specified in the string. In a future revision, we may
    // want to be fancier here by accepting formats that don't have various components (missing
    // year/month, for example). It seems like if we don't have eg year but the granularity is only
    // in seconds, then it should be perfectly possible to still form buckets. However, if we were
    // to do that we'd need to consider things like how we print out buckets when they're not really
    // 'full' DateTimes - just accept 0s for missing components?
    fn try_parse(&self, text: &str) -> chrono::format::ParseResult<DateTime<Utc>> {
        let mut parsed = Parsed::new();
        chrono::format::parse(
            &mut parsed,
            text,
            self.chrono_items.iter().map(FormatItem::to_chrono),
        )?;
        parsed.to_datetime_with_timezone(&Utc {})
    }

    // Determines whether there is enough information in the user's format string to satisfy chrono's
    // parser. This works by building up a dummy string that matches the user's specification
    // (substituting dummy values like 0001 for the year, etc), then trying to parse it.
    fn has_enough_info(&self) -> bool {
        let mut default_values = String::with_capacity(128);
        for item in &self.chrono_items {
            match item {
                FormatItem::Literal(string) | FormatItem::Space(string) => {
                    default_values.push_str(string);
                }
                FormatItem::Numeric(numeric, pad) => {
                    default_values.push_str(
                        numeric_format_to_default_value(numeric, *pad)
                            .expect("validator should have rejected unsupported items"),
                    );
                }
                FormatItem::Fixed(fixed) => {
                    default_values.push_str(
                        fixed_format_to_default_value(fixed)
                            .expect("validator should have rejected unsupported items"),
                    );
                }
            }
        }
        self.try_parse(&default_values).is_ok()
    }
}

// Convert a Numeric chrono specifier (like "%Y") into a regex fragment that will match values of
// that kind. Currently ignores the padding info - is there a case where doing so is incorrect?
fn numeric_format_to_regex_fragment(numeric: &Numeric, _pad: Pad) -> Option<&'static str> {
    use Numeric::*;
    Some(match numeric {
        Year => "-?\\d+",
        Month | Day | Hour | Hour12 | Minute | Second => "\\d{2}",
        Timestamp => "\\d+",
        _ => return None,
    })
}

// Get a dummy value for a chrono Numeric specifier.
fn numeric_format_to_default_value(numeric: &Numeric, _pad: Pad) -> Option<&'static str> {
    use Numeric::*;
    Some(match numeric {
        Year => "0001",
        Month | Day | Hour12 => "01",
        Hour | Minute | Second => "00",
        Timestamp => "000000000",
        _ => return None,
    })
}

// Convert a Fixed chrono specifier (like "%b") into a regex fragment that will match values of
// that kind.
fn fixed_format_to_regex_fragment(fixed: &Fixed) -> Option<&'static str> {
    use Fixed::*;
    Some(match fixed {
        ShortMonthName => "Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec",
        LongMonthName => "Jan(uary)?|Feb(ruary)?|Mar(ch)?|Apr(il)?|May|June?|July?|Aug(ust)?|Sep(tember)?|Oct(ober)?|Nov(ember)?|Dec(ember)?",
        LowerAmPm | UpperAmPm => "am|AM|pm|PM",
        _ => return None
    })
}

// Get a dummy value for a chrono Fixed specifier.
fn fixed_format_to_default_value(fixed: &Fixed) -> Option<&'static str> {
    use Fixed::*;
    Some(match fixed {
        ShortMonthName => "Jan",
        LongMonthName => "January",
        LowerAmPm => "am",
        UpperAmPm => "AM",
        _ => return None,
    })
}

#[cfg(test)]
mod datetime_format_tests {
    use super::DateTimeFormat;
    use chrono::{Datelike, Timelike};

    #[test]
    fn formats_are_matched() {
        let cases = vec![
            ("%Y", vec!["2019", "1", "0100", "100", "-1"]),
            (
                "%m",
                vec![
                    "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
                ],
            ),
            (
                "%b",
                vec![
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ],
            ),
            (
                "%B",
                vec![
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December",
                ],
            ),
            ("%d", vec!["01", "02", "10", "22", "31"]),
            ("%F", vec!["1991-08-10", "2019-03-14"]),
            ("%H", vec!["00", "02", "10", "19", "23"]),
            ("%I", vec!["01", "02", "05", "10", "12"]),
            ("%M", vec!["00", "02", "10", "19", "30", "44", "59"]),
            ("%S", vec!["00", "02", "10", "19", "30", "44", "59", "60"]),
            ("%T", vec!["00:00:00", "10:20:30", "23:59:60"]),
            ("%p", vec!["AM", "PM"]),
            ("%P", vec!["am", "pm"]),
            ("%s", vec!["994518299"]),
        ];
        for (strftime, expected_matches) in &cases {
            let format = DateTimeFormat::new(strftime).unwrap();
            let regex = format.regex();
            for expected_match in expected_matches {
                assert!(regex.is_match(expected_match));
            }
        }
    }

    #[test]
    fn has_enough_info() {
        let cases = vec!["%Y-%m-%d %H:%M:%S", "%F %T", "%b %d, %Y %I:%M %p"];
        for strftime in &cases {
            let format = DateTimeFormat::new(strftime).unwrap();
            assert!(format.has_enough_info());
        }
    }

    #[test]
    fn parses() {
        let cases = vec![
            (
                "%Y-%m-%d %H:%M:%S",
                "1991-08-10 01:02:03",
                1991,
                8,
                10,
                1,
                2,
                3,
            ),
            (
                "%b %d, %Y %I:%M:%S%P",
                "Mar 14, 2019 04:59:34pm",
                2019,
                3,
                14,
                16,
                59,
                34,
            ),
            ("%s", "1552609482", 2019, 3, 15, 00, 24, 42),
        ];
        for (strftime, text, y, mo, d, h, mi, s) in cases {
            let format = DateTimeFormat::new(strftime).unwrap();
            let datetime = format.try_parse(text).unwrap();
            let date = datetime.date();
            let time = datetime.time();
            assert_eq!(y, date.year());
            assert_eq!(mo, date.month());
            assert_eq!(d, date.day());
            assert_eq!(h, time.hour());
            assert_eq!(mi, time.minute());
            assert_eq!(s, time.second());
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Granularity {
    Second(NonZeroU32),
    Minute(NonZeroU32),
    Hour(NonZeroU32),
}

impl Granularity {
    fn parse(text: &str) -> Option<Self> {
        if let Some(index) = text.find('s') {
            text.split_at(index)
                .0
                .parse::<u32>()
                .ok()
                .and_then(NonZeroU32::new)
                .map(Granularity::Second)
        } else if let Some(index) = text.find('m') {
            text.split_at(index)
                .0
                .parse::<u32>()
                .ok()
                .and_then(NonZeroU32::new)
                .map(Granularity::Minute)
        } else if let Some(index) = text.find('h') {
            text.split_at(index)
                .0
                .parse::<u32>()
                .ok()
                .and_then(NonZeroU32::new)
                .map(Granularity::Hour)
        } else {
            None
        }
    }

    fn bucketize(&self, datetime: &DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Granularity::Second(s) => {
                let s = s.get();
                let time = datetime.time();
                datetime
                    .date()
                    .and_hms(time.hour(), time.minute(), time.second() / s * s)
            }
            Granularity::Minute(m) => {
                let m = m.get();
                let time = datetime.time();
                datetime
                    .date()
                    .and_hms(time.hour(), time.minute() / m * m, 0)
            }
            Granularity::Hour(h) => {
                let h = h.get();
                let time = datetime.time();
                datetime.date().and_hms(time.hour() / h * h, 0, 0)
            }
        }
    }

    fn successor(&self, datetime: &DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Granularity::Second(s) => *datetime + Duration::seconds(i64::from(s.get())),
            Granularity::Minute(m) => *datetime + Duration::minutes(i64::from(m.get())),
            Granularity::Hour(h) => *datetime + Duration::hours(i64::from(h.get())),
        }
    }
}

#[cfg(test)]
mod granularity_tests {
    use super::Granularity;
    use chrono::naive::NaiveDate;
    use chrono::{DateTime, Timelike, Utc};
    use std::num::NonZeroU32;

    #[test]
    fn parses() {
        let cases = vec![
            ("1s", Granularity::Second(NonZeroU32::new(1).unwrap())),
            ("5s", Granularity::Second(NonZeroU32::new(5).unwrap())),
            ("1m", Granularity::Minute(NonZeroU32::new(1).unwrap())),
            ("3m", Granularity::Minute(NonZeroU32::new(3).unwrap())),
            ("1h", Granularity::Hour(NonZeroU32::new(1).unwrap())),
            ("10h", Granularity::Hour(NonZeroU32::new(10).unwrap())),
        ];
        for (input, expected) in cases {
            assert_eq!(Granularity::parse(input).unwrap(), expected);
        }
    }

    #[test]
    fn bad_parses() {
        let cases = vec!["1", "-1s", "m"];
        for input in cases {
            assert!(Granularity::parse(input).is_none());
        }
    }

    #[test]
    fn bucketize() {
        for granularity_seconds in 1..100 {
            let granularity = Granularity::Second(NonZeroU32::new(granularity_seconds).unwrap());
            for input_second in 0..60 {
                let expected_bucket_second =
                    input_second / granularity_seconds * granularity_seconds;
                let input = DateTime::from_utc(
                    NaiveDate::from_ymd(1991, 8, 10).and_hms(10, 30, input_second),
                    Utc {},
                );
                let bucket = granularity.bucketize(&input);
                assert!(bucket.time().second() % granularity_seconds == 0);
                assert_eq!(expected_bucket_second, bucket.time().second());
            }
        }

        for granularity_minutes in 1..100 {
            let granularity = Granularity::Minute(NonZeroU32::new(granularity_minutes).unwrap());
            for input_minute in 0..60 {
                let expected_bucket_minute =
                    input_minute / granularity_minutes * granularity_minutes;
                let input = DateTime::from_utc(
                    NaiveDate::from_ymd(1991, 8, 10).and_hms(10, input_minute, 15),
                    Utc {},
                );
                let bucket = granularity.bucketize(&input);
                assert!(bucket.time().minute() % granularity_minutes == 0);
                assert_eq!(expected_bucket_minute, bucket.time().minute());
                assert_eq!(0, bucket.time().second());
            }
        }

        for granularity_hours in 1..100 {
            let granularity = Granularity::Hour(NonZeroU32::new(granularity_hours).unwrap());
            for input_hour in 0..24 {
                let expected_bucket_hour = input_hour / granularity_hours * granularity_hours;
                let input = DateTime::from_utc(
                    NaiveDate::from_ymd(1991, 8, 10).and_hms(input_hour, 43, 15),
                    Utc {},
                );
                let bucket = granularity.bucketize(&input);
                assert!(bucket.time().hour() % granularity_hours == 0);
                assert_eq!(expected_bucket_hour, bucket.time().hour());
                assert_eq!(0, bucket.time().second());
                assert_eq!(0, bucket.time().minute());
            }
        }
    }
}

// Owned equivalent of chrono::format::Item.
#[derive(Debug)]
enum FormatItem {
    Literal(String),
    Space(String),
    Numeric(Numeric, Pad),
    Fixed(Fixed),
}

impl FormatItem {
    // Convert from chrono's Item to ours. Allocates string slices into owned strings.
    fn from_chrono(item: Item<'_>) -> Self {
        use chrono::format::Item::*;
        match item {
            Literal(str_slice) => FormatItem::Literal(str_slice.to_string()),
            OwnedLiteral(box_str) => FormatItem::Literal(box_str.to_string()),
            Space(str_slice) => FormatItem::Space(str_slice.to_string()),
            OwnedSpace(box_str) => FormatItem::Space(box_str.to_string()),
            Numeric(numeric, pad) => FormatItem::Numeric(numeric, pad),
            Fixed(fixed) => FormatItem::Fixed(fixed),
            Error => unimplemented!(),
        }
    }

    // Convert back to chrono's representation. Needed for parsing.
    fn to_chrono(&self) -> Item {
        match self {
            FormatItem::Literal(string) => Item::Literal(string.as_str()),
            FormatItem::Space(string) => Item::Space(string.as_str()),
            FormatItem::Numeric(numeric, pad) => Item::Numeric(numeric.clone(), *pad),
            FormatItem::Fixed(fixed) => Item::Fixed(fixed.clone()),
        }
    }
}
