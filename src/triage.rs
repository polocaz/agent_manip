//! Agent log-line triage: parse the agent's log format and group error lines
//! by the source site that emitted them.
//!
//! Agent log line format (threadLog.cpp / LsiTrace.cpp):
//! `MM-DD HH:MM:SS <file>(<line>) -<L> <thread> <msg>` — timestamps are UTC
//! (gmtime, no year), level char from `" EWI12345"`.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use serde::Serialize;

/// One parsed agent log line. Fields are `None` when the line doesn't follow
/// the standard format (continuation lines, foreign output).
#[derive(Debug, Clone)]
pub struct ParsedLine {
    /// UTC timestamp (year inferred — the agent doesn't log one).
    pub timestamp: Option<NaiveDateTime>,
    /// Source site `<file>(<line>)`, e.g. `webSocNix.cpp(120)`.
    pub site: Option<String>,
    /// Level char: E, W, I, or 1-5 for trace levels.
    pub level: Option<char>,
}

/// Matches the first-pass triage pattern from the agent bug-investigation
/// runbook: explicit error-level lines (" -E ") plus crash-ish keywords.
pub fn is_error_line(line: &str) -> bool {
    if line.contains(" -E ") {
        return true;
    }
    let lower = line.to_lowercase();
    ["crash", "abort", "exception", "failed"]
        .iter()
        .any(|kw| lower.contains(kw))
}

/// Parse the leading `MM-DD HH:MM:SS <file>(<line>) -<L>` of an agent log
/// line. `now` supplies the year (and lets tests be deterministic): a
/// month/day that would land in the future is shifted back one year, so logs
/// spanning a New Year still order correctly.
pub fn parse_line(line: &str, now: DateTime<Utc>) -> ParsedLine {
    let mut parsed = ParsedLine { timestamp: None, site: None, level: None };

    let mut tokens = line.split_whitespace();
    let (Some(date_tok), Some(time_tok)) = (tokens.next(), tokens.next()) else {
        return parsed;
    };

    parsed.timestamp = parse_timestamp(date_tok, time_tok, now);
    if parsed.timestamp.is_none() {
        return parsed; // not a standard agent line; don't guess at the rest
    }

    if let Some(site_tok) = tokens.next() {
        parsed.site = site_token(site_tok);
    }

    if let Some(level_tok) = tokens.next() {
        let bytes = level_tok.as_bytes();
        if bytes.len() == 2 && bytes[0] == b'-' {
            let c = bytes[1] as char;
            if matches!(c, 'E' | 'W' | 'I' | '1'..='5') {
                parsed.level = Some(c);
            }
        }
    }

    parsed
}

fn parse_timestamp(date_tok: &str, time_tok: &str, now: DateTime<Utc>) -> Option<NaiveDateTime> {
    let (month, day) = date_tok.split_once('-')?;
    let (month, day) = (month.parse::<u32>().ok()?, day.parse::<u32>().ok()?);

    let mut parts = time_tok.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let min = parts.next()?.parse::<u32>().ok()?;
    let sec = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let this_year = NaiveDate::from_ymd_opt(now.year(), month, day)?
        .and_hms_opt(hour, min, sec)?;
    // A "future" stamp means the log line predates the current year
    // (e.g. reading December's log in January).
    if this_year > now.naive_utc() + chrono::Duration::days(1) {
        return NaiveDate::from_ymd_opt(now.year() - 1, month, day)?.and_hms_opt(hour, min, sec);
    }
    Some(this_year)
}

/// Normalize a token shaped like `<file>(<line>)` (the file is logged without
/// its extension, e.g. `webSocNix(120)`). The agent pads this into a
/// fixed-width column, so long file names lose the closing paren and even
/// line-number digits (`threadStatusBarBridge(75`); such truncated sites are
/// kept, marked with a trailing `…`.
fn site_token(token: &str) -> Option<String> {
    let open = token.find('(')?;
    if open == 0 {
        return None;
    }
    let inner = &token[open + 1..];
    let (digits, complete) = match inner.strip_suffix(')') {
        Some(d) => (d, true),
        None => (inner, false),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(if complete {
        token.to_string()
    } else {
        format!("{}…", token)
    })
}

/// Split a source site `webSocNix(120)` into (`webSocNix`, 120). For a
/// column-truncated site (`threadStatusBarBridge(75…`) the digits are only a
/// prefix of the real line number, so the line comes back as 0 (unknown).
pub fn split_site(site: &str) -> Option<(&str, u32)> {
    let open = site.find('(')?;
    if open == 0 {
        return None;
    }
    let file = &site[..open];
    let inner = site[open + 1..].trim_end_matches('…');
    match inner.strip_suffix(')') {
        Some(digits) => Some((file, digits.parse().ok()?)),
        None => {
            if inner.is_empty() || !inner.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((file, 0))
        }
    }
}

/// Number of trend buckets a windowed scan spreads each group's hits over.
pub const TREND_BUCKETS: usize = 40;

/// Error lines aggregated by the source site that emitted them.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorGroup {
    /// Short log name(s) the group was seen in (e.g. "agent").
    pub log: String,
    /// `<file>(<line>)` source site, or "(unrecognized format)".
    pub site: String,
    pub count: usize,
    /// First/last UTC timestamps seen, as `MM-DD HH:MM:SS` strings.
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    /// Most recent matching line, whitespace-trimmed.
    pub sample: String,
    /// Hits per time bucket across the scan window ([`TREND_BUCKETS`] cells,
    /// oldest first). Empty when the scan had no window (whole logs).
    pub buckets: Vec<u32>,
}

/// Streaming accumulator: feed every error line, get groups sorted by
/// recency (most recently seen site first), ties broken by count.
#[derive(Default)]
pub struct ErrorGrouper {
    groups: std::collections::HashMap<(String, String), ErrorGroup>,
    /// `(window start, window length in seconds)` for trend bucketing.
    window: Option<(DateTime<Utc>, i64)>,
}

impl ErrorGrouper {
    pub fn new() -> Self {
        Self::default()
    }

    /// A grouper that also builds per-group trend buckets over
    /// `[now - duration, now]`.
    pub fn with_window(now: DateTime<Utc>, duration: chrono::Duration) -> Self {
        Self {
            groups: std::collections::HashMap::new(),
            window: Some((now - duration, duration.num_seconds().max(1))),
        }
    }

    pub fn add(&mut self, log_name: &str, line: &str, now: DateTime<Utc>) {
        let parsed = parse_line(line, now);
        let site = parsed
            .site
            .unwrap_or_else(|| "(unrecognized format)".to_string());
        let ts = parsed.timestamp.map(|t| t.format("%m-%d %H:%M:%S").to_string());
        let bucket = self.window.and_then(|(start, total)| {
            let off = (parsed.timestamp? - start.naive_utc()).num_seconds();
            (0..total).contains(&off).then(|| {
                ((off as usize).saturating_mul(TREND_BUCKETS) / total as usize)
                    .min(TREND_BUCKETS - 1)
            })
        });
        let windowed = self.window.is_some();

        let group = self
            .groups
            .entry((log_name.to_string(), site.clone()))
            .or_insert_with(|| ErrorGroup {
                log: log_name.to_string(),
                site,
                count: 0,
                first_seen: ts.clone(),
                last_seen: None,
                sample: String::new(),
                buckets: if windowed { vec![0; TREND_BUCKETS] } else { Vec::new() },
            });
        group.count += 1;
        if group.first_seen.is_none() {
            group.first_seen = ts.clone();
        }
        if ts.is_some() {
            group.last_seen = ts;
        }
        if let Some(b) = bucket {
            group.buckets[b] += 1;
        }
        group.sample = line.trim().to_string();
    }

    pub fn total(&self) -> usize {
        self.groups.values().map(|g| g.count).sum()
    }

    /// Groups sorted most-recently-seen first (unstamped groups last),
    /// ties broken by count descending.
    pub fn into_sorted(self) -> Vec<ErrorGroup> {
        let mut groups: Vec<ErrorGroup> = self.groups.into_values().collect();
        groups.sort_by(|a, b| {
            b.last_seen
                .cmp(&a.last_seen)
                .then(b.count.cmp(&a.count))
                .then(a.site.cmp(&b.site))
        });
        groups
    }
}

/// Parse a duration like `90s`, `30m`, `24h`, `7d` for `--since` filters.
pub fn parse_since(s: &str) -> Option<chrono::Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    if n < 0 {
        return None;
    }
    match unit {
        "s" => Some(chrono::Duration::seconds(n)),
        "m" => Some(chrono::Duration::minutes(n)),
        "h" => Some(chrono::Duration::hours(n)),
        "d" => Some(chrono::Duration::days(n)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0).unwrap()
    }

    #[test]
    fn parses_full_line() {
        let p = parse_line(
            "07-04 09:15:42 webSocNix.cpp(120) -E WebSock connect to master failed",
            now(),
        );
        assert_eq!(
            p.timestamp.unwrap().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-07-04 09:15:42"
        );
        assert_eq!(p.site.as_deref(), Some("webSocNix.cpp(120)"));
        assert_eq!(p.level, Some('E'));
    }

    #[test]
    fn future_month_rolls_back_a_year() {
        let p = parse_line("12-25 10:00:00 threadColl.cpp(88) -I snapshot", now());
        assert_eq!(p.timestamp.unwrap().year(), 2025);
    }

    #[test]
    fn nonstandard_line_yields_nones() {
        let p = parse_line("   at libsystem_kernel.dylib + 123", now());
        assert!(p.timestamp.is_none());
        assert!(p.site.is_none());
        assert!(p.level.is_none());
    }

    #[test]
    fn site_detection() {
        assert_eq!(site_token("webSocNix(120)"), Some("webSocNix(120)".to_string()));
        // fixed-width column truncation loses the closing paren
        assert_eq!(
            site_token("threadStatusBarBridge(75"),
            Some("threadStatusBarBridge(75…".to_string())
        );
        assert_eq!(site_token("(120)"), None);
        assert_eq!(site_token("no-parens"), None);
        assert_eq!(site_token("bad(12x)"), None);
        assert_eq!(split_site("dbConnNix.cpp(310)"), Some(("dbConnNix.cpp", 310)));
        // truncated line digits are unreliable -> line 0
        assert_eq!(
            split_site("threadStatusBarBridge(75…"),
            Some(("threadStatusBarBridge", 0))
        );
        assert_eq!(split_site("nope"), None);
    }

    #[test]
    fn parses_real_agent_line_padding() {
        // real lines pad the site column and may carry an empty thread name
        let p = parse_line(
            "07-02 12:13:32 DbUtils(5009)            -E        Failed to create CDbSaVerRs during upgrade",
            now(),
        );
        assert_eq!(p.site.as_deref(), Some("DbUtils(5009)"));
        assert_eq!(p.level, Some('E'));
    }

    #[test]
    fn grouping_counts_and_orders() {
        let mut g = ErrorGrouper::new();
        g.add("agent", "07-01 08:00:00 a.cpp(1) -E first failure", now());
        g.add("agent", "07-02 08:00:00 a.cpp(1) -E second failure", now());
        g.add("agent", "07-03 08:00:00 b.cpp(2) -E newer but rarer", now());
        assert_eq!(g.total(), 3);

        let groups = g.into_sorted();
        assert_eq!(groups.len(), 2);
        // most recently seen first
        assert_eq!(groups[0].site, "b.cpp(2)");
        assert_eq!(groups[1].site, "a.cpp(1)");
        assert_eq!(groups[1].count, 2);
        assert_eq!(groups[1].first_seen.as_deref(), Some("07-01 08:00:00"));
        assert_eq!(groups[1].last_seen.as_deref(), Some("07-02 08:00:00"));
        assert_eq!(groups[1].sample, "07-02 08:00:00 a.cpp(1) -E second failure");
    }

    #[test]
    fn windowed_grouping_fills_trend_buckets() {
        // now = 07-05 12:00; 24h window starts 07-04 12:00
        let mut g = ErrorGrouper::with_window(now(), chrono::Duration::hours(24));
        g.add("agent", "07-04 12:30:00 a.cpp(1) -E early", now()); // ~2% in
        g.add("agent", "07-05 11:30:00 a.cpp(1) -E late", now()); // ~98% in
        g.add("agent", "07-05 11:31:00 a.cpp(1) -E late again", now());
        let groups = g.into_sorted();
        assert_eq!(groups.len(), 1);
        let b = &groups[0].buckets;
        assert_eq!(b.len(), TREND_BUCKETS);
        assert_eq!(b.iter().sum::<u32>(), 3);
        assert_eq!(b[0], 1, "early hit lands in the first bucket");
        assert_eq!(b[TREND_BUCKETS - 1], 2, "late hits land in the last bucket");

        // unwindowed groupers keep buckets empty
        let mut g = ErrorGrouper::new();
        g.add("agent", "07-04 12:30:00 a.cpp(1) -E x", now());
        assert!(g.into_sorted()[0].buckets.is_empty());
    }

    #[test]
    fn since_parsing() {
        assert_eq!(parse_since("24h"), Some(chrono::Duration::hours(24)));
        assert_eq!(parse_since("30m"), Some(chrono::Duration::minutes(30)));
        assert_eq!(parse_since("7d"), Some(chrono::Duration::days(7)));
        assert_eq!(parse_since("90s"), Some(chrono::Duration::seconds(90)));
        assert!(parse_since("bogus").is_none());
        assert!(parse_since("").is_none());
    }

    #[test]
    fn error_line_detection() {
        assert!(is_error_line("07-04 12:00:00 webSocNix.cpp(120) -E WebSock connect failed"));
        assert!(is_error_line("something Exception thrown"));
        assert!(!is_error_line("07-04 12:00:00 threadColl.cpp(88) -I Coll snapshot ok"));
    }
}
