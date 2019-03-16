# tbuck - timeseries bucketing

[![Crates.io](https://img.shields.io/crates/v/tbuck.svg)](https://crates.io/crates/tbuck)
[![Docs](https://docs.rs/tbuck/badge.svg)](https://docs.rs/crate/tbuck/)
[![License](https://img.shields.io/crates/l/tbuck.svg)](https://creativecommons.org/share-your-work/public-domain/cc0/)
[![Last Commit](https://img.shields.io/github/last-commit/EkardNT/tbuck/master.svg)](https://github.com/EkardNT/tbuck/commits/master)
[![Build Status](https://travis-ci.org/EkardNT/tbuck.svg?branch=master)](https://travis-ci.org/EkardNT/tbuck)
[![Build status](https://ci.appveyor.com/api/projects/status/9deap878x2h3rj17/branch/master?svg=true)](https://ci.appveyor.com/project/EkardNT/tbuck/branch/master)

**tbuck** is a simple CLI tool allows you to take lines of text, group them into buckets according to some time granularity, and emit the count of occurrences for each bucket. My motivation for writing it was that I found myself debugging an issue for work where I was trying to find how often a particular event was occurring, identified by a line in an application's log file. The event did not correspond to any metric being emitted into our monitoring system, but I wanted to see a graph of how often the event was occurring. This requirement came up multiple times for multiple different formats of files during the investigation, and I wrote a per-format script for each case. Finally I realized that all the scripts were doing basically the same thing, and wrote tbuck.

## Usage

```
tbuck 1.0.4
Drake Tetreault <ekardnt@ekardnt.com>
A command line tool for bucketing time-series text data

USAGE:
    tbuck [FLAGS] [OPTIONS] <DATE_TIME_FORMAT> [INPUT_FILE]...

FLAGS:
    -d, --descending
            By default stream mode expects entries to be in monotonically ascending order by date (earlier dates
            followed by later dates), which is the usual order of log files. If this flag is present then stream mode
            will instead expect entries in monotonically decreasing order by date (later dates followed by earlier
            dates). In normal mode, this flag will cause the buckets to be printed in descending order instead of the
            default ascending order.
    -h, --help
            Prints help information

    -n, --no-fill
            By default buckets which had no entries present will be displayed with a count of 0. If this flag is present
            then instead the bucket will not be printed at all.
    -s, --stream
            Enable stream mode. Entries will be expected to arrive in monotonically increasing (or --decreasing) order,
            and bucket information will be printed live as soon as the bucket is known to be finished. By default the
            presence of any entry violating the monotonic order will cause an error, but this can be made --tolerant.
    -t, --tolerant
            By default when a non-monotonic entry is encountered in stream mode the program will terminate with an
            error. If this flag is present then non-monotonic entries will instead be silently discarded.
    -V, --version
            Prints version information


OPTIONS:
    -g, --granularity <GRANULARITY>
            Bucket time granularity in seconds ('5s'), minutes ('1m'), or hours ('2h') [default: 1m]

    -m, --match-index <MATCH_INDEX>
            0-based index of match to use if multiple matches are found [default: 0]


ARGS:
    <DATE_TIME_FORMAT>
            Date/time parsing format. Full date and time information must be present. The following specifiers are
            supported, taken from Rust's chrono crate:
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
            %s          994518299   UNIX timestamp, the number of seconds since 1970-01-01 00:00 UTC.
    <INPUT_FILE>...
            Input files; or standard input if none provided
```

## Example

Suppose you're working with the following log file.

```
$ cat demo.txt
2019-03-14 12:01:00 Event A
2019-03-14 12:01:10 Event B
2019-03-14 12:01:20 Event A
2019-03-14 12:01:30 Event B
2019-03-14 12:01:40 Event A
2019-03-14 12:01:50 Event B
2019-03-14 12:02:00 Event A
2019-03-14 12:02:10 Event B
2019-03-14 12:02:20 Event A
2019-03-14 12:02:30 Event B
2019-03-14 12:02:40 Event A
2019-03-14 12:02:50 Event B
2019-03-14 12:03:00 Event A
2019-03-14 12:03:10 Event B
2019-03-14 12:03:20 Event A
2019-03-14 12:03:30 Event B
2019-03-14 12:03:40 Event A
2019-03-14 12:03:50 Event B
```

You want to see how many log lines there are for every 1-minute bucket in the file.

```
$ tbuck --granularity 1m '%F %T' demo.txt
2019-03-14 12:01:00 UTC,6
2019-03-14 12:02:00 UTC,6
2019-03-14 12:03:00 UTC,6
```

You want to see how many log lines there are for every 30-second bucket in the file. Note that from now on, these examples will use the short form `-g` of the `--granularity` argument.

```
$ tbuck -g 30s '%F %T' demo.txt
2019-03-14 12:01:00 UTC,3
2019-03-14 12:01:30 UTC,3
2019-03-14 12:02:00 UTC,3
2019-03-14 12:02:30 UTC,3
2019-03-14 12:03:00 UTC,3
2019-03-14 12:03:30 UTC,3
```

You want to see how many log lines of event A there are for every 15-second bucket in the file. `rg` is [ripgrep](https://github.com/BurntSushi/ripgrep).

```
$rg "Event A" demo.txt | tbuck -g 15s '%F %T'
2019-03-14 12:01:00 UTC,1
2019-03-14 12:01:15 UTC,1
2019-03-14 12:01:30 UTC,1
2019-03-14 12:01:45 UTC,0
2019-03-14 12:02:00 UTC,1
2019-03-14 12:02:15 UTC,1
2019-03-14 12:02:32019-03-14 12:02:45 UTC,00 UTC,1
2019-03-14 12:02:45 UTC,0
2019-03-14 12:03:00 UTC,1
2019-03-14 12:03:15 UTC,1
2019-03-14 12:03:30 UTC,1
```

You noticed that the previous command printed 0s for buckets without any entries that fell within them, and you don't want that for some reason.

```
$rg "Event A" demo.txt | tbuck -g 15s --no-fill '%F %T'
2019-03-14 12:01:00 UTC,1
2019-03-14 12:01:15 UTC,1
2019-03-14 12:01:30 UTC,1
2019-03-14 12:02:00 UTC,1
2019-03-14 12:02:15 UTC,1
2019-03-14 12:02:30 UTC,1
2019-03-14 12:03:00 UTC,1
2019-03-14 12:03:15 UTC,1
2019-03-14 12:03:30 UTC,1
```