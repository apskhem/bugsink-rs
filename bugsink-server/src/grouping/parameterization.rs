//! Port of upstream's vendored Sentry message parameterization (`sentry/at_597d25951d00/grouping/`), used by the
//! `bugsink-v2` grouper: replaces emails, URLs, IPs, ids, numbers, ... with `<email>`, `<int>` etc.
//!
//! The patterns are kept verbatim from upstream, in Python's verbose regex syntax, and stripped of verbose-mode
//! whitespace/comments here (Rust's verbose mode treats whitespace inside character classes differently). They need
//! lookbehind, hence `fancy_regex`.

use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::py::{
  is_space,
  splitlines,
};

struct Pattern {
  name: &'static str,
  raw: &'static str,
  lookbehind: Option<&'static str>,
}

const PATTERNS: &[Pattern] = &[
  Pattern {
    name: "email",
    raw: r#"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*"#,
    lookbehind: None,
  },
  Pattern {
    name: "url",
    raw: r#"\b(wss?|https?|ftp)://[^\s/$.?#].[^\s]*"#,
    lookbehind: None,
  },
  Pattern {
    name: "hostname",
    raw: r#"
            # Top 100 TLDs. The complete list is 1000s long.
            \b
            ([a-zA-Z0-9\-]{1,63}\.)+?
            (
                (COM|NET|ORG|JP|DE|UK|FR|BR|IT|RU|ES|ME|GOV|PL|CA|AU|CN|CO|IN|NL|EDU|INFO|EU|CH|ID|AT|KR|CZ|MX|BE|TV|SE|TR|TW|AL|UA|IR|VN|CL|SK|LY|CC|TO|NO|FI|US|PT|DK|AR|HU|TK|GR|IL|NEWS|RO|MY|BIZ|IE|ZA|NZ|SG|EE|TH|IO|XYZ|PE|BG|HK|RS|LT|LINK|PH|CLUB|SI|SITE|MOBI|BY|CAT|WIKI|LA|GA|XXX|CF|HR|NG|JOBS|ONLINE|KZ|UG|GQ|AE|IS|LV|PRO|FM|TIPS|MS|SA|APP)|
                (com|net|org|jp|de|uk|fr|br|it|ru|es|me|gov|pl|ca|au|cn|co|in|nl|edu|info|eu|ch|id|at|kr|cz|mx|be|tv|se|tr|tw|al|ua|ir|vn|cl|sk|ly|cc|to|no|fi|us|pt|dk|ar|hu|tk|gr|il|news|ro|my|biz|ie|za|nz|sg|ee|th|io|xyz|pe|bg|hk|rs|lt|link|ph|club|si|site|mobi|by|cat|wiki|la|ga|xxx|cf|hr|ng|jobs|online|kz|ug|gq|ae|is|lv|pro|fm|tips|ms|sa|app)
            )
            \b
        "#,
    lookbehind: None,
  },
  Pattern {
    name: "ip",
    raw: r#"
            (
                ([0-9a-fA-F]{1,4}:){7,7}[0-9a-fA-F]{1,4}|
                ([0-9a-fA-F]{1,4}:){1,7}:|
                ([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|
                ([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|
                ([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|
                ([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|
                ([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|
                [0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|
                :((:[0-9a-fA-F]{1,4}){1,7}|:)|
                fe80:(:[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|
                ::(ffff(:0{1,4}){0,1}:){0,1}
                ((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}
                (25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])|
                ([0-9a-fA-F]{1,4}:){1,4}:
                ((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}
                (25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\b
            ) |
            (
                \b((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}
                (25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\b
            )
        "#,
    lookbehind: None,
  },
  Pattern {
    name: "uuid",
    raw: r#"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "sha1",
    raw: r#"\b[0-9a-fA-F]{40}\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "md5",
    raw: r#"\b[0-9a-fA-F]{32}\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "date",
    raw: r#"
            # No word boundaries required around dates. Should there be?
            # RFC822, RFC1123, RFC1123Z
            ((?:Sun|Mon|Tue|Wed|Thu|Fri|Sat),\s\d{1,2}\s(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s\d{2,4}\s\d{1,2}:\d{1,2}(:\d{1,2})?\s([-\+][\d]{2}[0-5][\d]|(?:UT|GMT|(?:E|C|M|P)(?:ST|DT)|[A-IK-Z])))
            |
            # Similar to RFC822, but "Mon Jan 02, 1999", "Jan 02, 1999"
            (((?:Sun|Mon|Tue|Wed|Thu|Fri|Sat)\s)?(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s[0-3]\d,\s\d{2,4})
            |
            # RFC850
            ((?:Sunday|Monday|Tuesday|Wednesday|Thursday|Friday|Saturday),\s\d{2}-(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)-\d{2}\s\d{2}:\d{2}:\d{2}\s(?:UT|GMT|(?:E|C|M|P)(?:ST|DT)|[A-IK-Z]))
            |
            # RFC3339, RFC3339Nano
            (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z?([+-]?\d{2}:\d{2})?)
            |
            # Datetime:
            (\d{4}-?[01]\d-?[0-3]\d\s[0-2]\d:[0-5]\d:[0-5]\d)(\.\d+)?
            |
            # Kitchen
            (\d{1,2}:\d{2}(:\d{2})?(?: [aApP][Mm])?)
            |
            # Date
            (\d{4}-[01]\d-[0-3]\d)
            |
            # Time
            ([0-2]\d:[0-5]\d:[0-5]\d)
            |
            # Old Date Formats, TODO: possibly safe to remove?
            (
                (\d{4}-[01]\d-[0-3]\dT[0-2]\d:[0-5]\d:[0-5]\d\.\d+([+-][0-2]\d:[0-5]\d|Z))|
                (\d{4}-[01]\d-[0-3]\dT[0-2]\d:[0-5]\d:[0-5]\d([+-][0-2]\d:[0-5]\d|Z))|
                (\d{4}-[01]\d-[0-3]\dT[0-2]\d:[0-5]\d([+-][0-2]\d:[0-5]\d|Z))
            ) |
            (
                \b(?:(Sun|Mon|Tue|Wed|Thu|Fri|Sat)\s+)?
                (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+
                ([\d]{1,2})\s+
                ([\d]{2}:[\d]{2}:[\d]{2})\s+
                [\d]{4}
            ) |
            (
                \b(?:(Sun|Mon|Tue|Wed|Thu|Fri|Sat),\s+)?
                (0[1-9]|[1-2]?[\d]|3[01])\s+
                (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+
                (19[\d]{2}|[2-9][\d]{3})\s+
                (2[0-3]|[0-1][\d]):([0-5][\d])
                (?::(60|[0-5][\d]))?\s+
                ([-\+][\d]{2}[0-5][\d]|(?:UT|GMT|(?:E|C|M|P)(?:ST|DT)|[A-IK-Z]))
            ) |
            (datetime.datetime\(.*?\))
        "#,
    lookbehind: None,
  },
  Pattern {
    name: "duration",
    raw: r#"\b(\d+ms) | (\d(\.\d+)?s)\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "hex",
    raw: r#"\b0[xX][0-9a-fA-F]+\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "float",
    raw: r#"-\d+\.\d+\b | \b\d+\.\d+\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "int",
    raw: r#"-\d+\b | \b\d+\b"#,
    lookbehind: None,
  },
  Pattern {
    name: "quoted_str",
    raw: r#"# Using `=`lookbehind which guarantees we'll only match the value half of key-value pairs,
            # rather than all quoted strings
            '([^']+)' | "([^"]+)"
        "#,
    lookbehind: Some("="),
  },
  Pattern {
    name: "bool",
    raw: r#"# Using `=`lookbehind which guarantees we'll only match the value half of key-value pairs,
            # rather than all instances of the words 'true' and 'false'.
            True |
            true |
            False |
            false
        "#,
    lookbehind: Some("="),
  },
];

static PARAMETERIZER: LazyLock<Regex> = LazyLock::new(|| {
  let combined = PATTERNS
    .iter()
    .map(|p| {
      let prefix = p.lookbehind.map(|lb| format!("(?<={lb})")).unwrap_or_default();
      format!("{prefix}(?P<{}>{})", p.name, p.raw)
    })
    .collect::<Vec<_>>()
    .join("|");
  Regex::new(&strip_verbose(&combined)).expect("parameterization regex compiles")
});

/// Removes what Python's `re.VERBOSE` ignores: whitespace and `#` comments outside character classes.
fn strip_verbose(pattern: &str) -> String {
  let mut out = String::with_capacity(pattern.len());
  let mut chars = pattern.chars();
  let mut in_class = false;
  while let Some(c) = chars.next() {
    match c {
      '\\' => {
        out.push(c);
        if let Some(next) = chars.next() {
          out.push(next);
        }
      }
      '[' if !in_class => {
        in_class = true;
        out.push(c);
      }
      ']' if in_class => {
        in_class = false;
        out.push(c);
      }
      '#' if !in_class => {
        for skipped in chars.by_ref() {
          if skipped == '\n' {
            break;
          }
        }
      }
      c if !in_class && c.is_whitespace() => {}
      c => out.push(c),
    }
  }
  out
}

/// `normalize_message_for_grouping`: first two non-blank lines (plus "..." if anything was cut), parameterized.
pub fn normalize_message_for_grouping(message: &str) -> String {
  let mut trimmed = splitlines(message)
    .into_iter()
    .filter(|line| line.chars().any(|c| !is_space(c)))
    .take(2)
    .collect::<Vec<_>>()
    .join("\n");
  if trimmed != message {
    trimmed.push_str("...");
  }
  parameterize(&trimmed)
}

fn parameterize(content: &str) -> String {
  let mut out = String::with_capacity(content.len());
  let mut last = 0;
  for caps in PARAMETERIZER.captures_iter(content) {
    let caps = match caps {
      Ok(caps) => caps,
      // Backtracking limit hit. Upstream (Python `re`) has no limit and would just take long; keeping the
      // remainder as-is is the closest we get without hanging.
      Err(_) => break,
    };
    let whole = caps.get(0).expect("group 0 always participates");
    out.push_str(&content[last..whole.start()]);
    if let Some(p) = PATTERNS.iter().find(|p| caps.name(p.name).is_some()) {
      out.push('<');
      out.push_str(p.name);
      out.push('>');
    }
    last = whole.end();
  }
  out.push_str(&content[last..]);
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parameterizes_like_sentry() {
    for (input, expected) in [
      ("User 17 not found", "User <int> not found"),
      ("mail to foo@example.com failed", "mail to <email> failed"),
      ("GET https://example.com/a?b=1 failed", "GET <url> failed"),
      ("connect to 10.0.0.1 refused", "connect to <ip> refused"),
      ("id 3f2504e0-4f89-11d3-9a0c-0305e82c3301", "id <uuid>"),
      ("took 1.5s and 30ms", "took <duration> and <duration>"),
      ("addr 0xdeadbeef", "addr <hex>"),
      ("x=1.25 y=-3", "x=<float> y=<int>"),
      ("key='abc' flag=true", "key=<quoted_str> flag=<bool>"),
      ("at 2024-01-02T03:04:05Z", "at <date>"),
    ] {
      assert_eq!(parameterize(input), expected, "{input}");
    }
  }

  #[test]
  fn normalizes_lines() {
    assert_eq!(normalize_message_for_grouping("a 1\n\nb 2\nc 3"), "a <int>\nb <int>...");
    assert_eq!(normalize_message_for_grouping("abc\n"), "abc...");
    assert_eq!(normalize_message_for_grouping(""), "");
  }
}
