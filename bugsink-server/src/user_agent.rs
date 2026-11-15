//! Browser/OS/device from a User-Agent string, exactly as upstream gets them: the `user-agents` package on
//! `ua-parser`'s legacy `user_agent_parser.Parse`, with the regexes of `ua-parser-builtins` (`ua_regexes.json`, exported
//! from the upstream 2.6.3 image). Python's `re` semantics that the legacy parser depends on are reproduced:
//! `match.lastindex`, `$N` replacement, and the replacement-template escapes of `re.sub`.

use std::collections::HashMap;
use std::sync::{
  LazyLock,
  Mutex,
};

use regex::{
  Regex,
  RegexBuilder,
};
use serde::Deserialize;
use serde_json::{
  Value,
  json,
};

/// What `user_agents.parse` gives, as far as upstream uses it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UserAgent {
  pub browser_family: String,
  pub browser_version: String,
  pub os_family: String,
  pub os_version: String,
  pub device_family: String,
  pub device_brand: Option<String>,
  pub device_model: Option<String>,
}

/// Inputs on which upstream's parsing raises (and its caller then leaves the contexts alone).
#[derive(Debug, PartialEq)]
pub(crate) struct ParseError(pub &'static str);

#[derive(Deserialize)]
struct Regexes {
  user_agent_parsers: Vec<UserAgentRule>,
  os_parsers: Vec<OsRule>,
  device_parsers: Vec<DeviceRule>,
}

#[derive(Deserialize)]
struct UserAgentRule {
  regex: String,
  family_replacement: Option<String>,
  v1_replacement: Option<String>,
  v2_replacement: Option<String>,
}

#[derive(Deserialize)]
struct OsRule {
  regex: String,
  os_replacement: Option<String>,
  os_v1_replacement: Option<String>,
  os_v2_replacement: Option<String>,
  os_v3_replacement: Option<String>,
  os_v4_replacement: Option<String>,
}

#[derive(Deserialize)]
struct DeviceRule {
  regex: String,
  regex_flag: Option<String>,
  device_replacement: Option<String>,
  brand_replacement: Option<String>,
  model_replacement: Option<String>,
}

/// A compiled pattern, with what's needed to compute Python's `match.lastindex`.
struct Pattern {
  regex: Regex,
  /// Capture group numbers ordered by the position of their closing parenthesis in the pattern.
  close_order: Vec<usize>,
}

struct Parsers {
  user_agent: Vec<(Pattern, UserAgentRule)>,
  os: Vec<(Pattern, OsRule)>,
  device: Vec<(Pattern, DeviceRule)>,
}

static PARSERS: LazyLock<Parsers> = LazyLock::new(|| {
  let regexes: Regexes = serde_json::from_str(include_str!("ua_regexes.json")).expect("ua_regexes.json");
  Parsers {
    user_agent: regexes
      .user_agent_parsers
      .into_iter()
      .map(|r| (Pattern::new(&r.regex, false), r))
      .collect(),
    os: regexes
      .os_parsers
      .into_iter()
      .map(|r| (Pattern::new(&r.regex, false), r))
      .collect(),
    device: regexes
      .device_parsers
      .into_iter()
      .map(|r| (Pattern::new(&r.regex, r.regex_flag.as_deref() == Some("i")), r))
      .collect(),
  }
});

impl Pattern {
  fn new(pattern: &str, case_insensitive: bool) -> Self {
    let regex = RegexBuilder::new(pattern)
      .case_insensitive(case_insensitive)
      .build()
      .unwrap_or_else(|e| panic!("ua regex {pattern:?}: {e}"));
    Self {
      regex,
      close_order: close_order(pattern),
    }
  }
}

/// Capture groups in the order their `)` appears. In a match, the group closed last (`lastindex`) is the
/// participating one that comes last in this order.
fn close_order(pattern: &str) -> Vec<usize> {
  let mut order = Vec::new();
  let mut open: Vec<Option<usize>> = Vec::new(); // group number, or None for non-capturing
  let mut next_group = 1;
  let mut chars = pattern.chars().peekable();
  let mut in_class = false;
  while let Some(c) = chars.next() {
    match c {
      '\\' => {
        chars.next();
      }
      '[' if !in_class => {
        in_class = true;
        // a `]` right after `[` or `[^` is literal
        if chars.peek() == Some(&'^') {
          chars.next();
        }
        if chars.peek() == Some(&']') {
          chars.next();
        }
      }
      ']' if in_class => in_class = false,
      '(' if !in_class => {
        let capturing = chars.peek() != Some(&'?') || pattern_is_named_group(&mut chars.clone());
        if capturing {
          open.push(Some(next_group));
          next_group += 1;
        } else {
          open.push(None);
        }
      }
      ')' if !in_class => {
        if let Some(Some(group)) = open.pop() {
          order.push(group);
        }
      }
      _ => {}
    }
  }
  order
}

/// `(?P<name>...)` captures; other `(?...)` constructs don't.
fn pattern_is_named_group(rest: &mut std::iter::Peekable<std::str::Chars>) -> bool {
  rest.next() == Some('?') && rest.next() == Some('P') && rest.next() == Some('<')
}

/// A successful `re.search` as the legacy parser sees it.
struct Match<'a> {
  groups: Vec<Option<&'a str>>, // index 0 is the whole match
  lastindex: Option<usize>,
}

impl<'a> Match<'a> {
  fn search(pattern: &Pattern, haystack: &'a str) -> Option<Self> {
    let captures = pattern.regex.captures(haystack)?;
    let groups: Vec<Option<&str>> = (0..captures.len())
      .map(|i| captures.get(i).map(|m| m.as_str()))
      .collect();
    let lastindex = pattern
      .close_order
      .iter()
      .rev()
      .copied()
      .find(|g| groups.get(*g).is_some_and(Option::is_some));
    Some(Self { groups, lastindex })
  }

  /// `match[i]`
  fn group(&self, i: usize) -> Option<&'a str> {
    self.groups.get(i).copied().flatten()
  }

  /// `len(match.groups())`
  fn group_count(&self) -> usize {
    self.groups.len() - 1
  }

  fn lastindex_at_least(&self, i: usize) -> bool {
    self.lastindex.is_some_and(|last| last >= i)
  }
}

/// `MultiReplace`: `$1`..`$9` become the groups (missing or non-participating ones: empty), then strip; empty is None.
fn multi_replace(template: &str, m: &Match) -> Option<String> {
  let mut out = String::new();
  let mut chars = template.chars().peekable();
  while let Some(c) = chars.next() {
    if c == '$'
      && let Some(d) = chars.peek().and_then(|d| d.to_digit(10))
    {
      chars.next();
      let index = d as usize; // `$0` is index -1 in Python: the last group
      let group = if index == 0 {
        m.group(m.group_count())
      } else {
        m.group(index)
      };
      if index <= m.group_count() {
        out.push_str(group.unwrap_or(""));
      }
      continue;
    }
    out.push(c);
  }
  let out = crate::py::strip(&out);
  (!out.is_empty()).then(|| out.to_string())
}

/// `re.sub(r"\$1", group1, family_replacement)`: `group1` is used as a replacement template, so its backslash escapes
/// are processed (and some are errors).
fn family_template(family_replacement: &str, group1: Option<&str>) -> Result<String, ParseError> {
  let group1 = group1.ok_or(ParseError("TypeError: replacement is None"))?;
  let mut expanded = String::new();
  let mut chars = group1.chars();
  while let Some(c) = chars.next() {
    if c != '\\' {
      expanded.push(c);
      continue;
    }
    match chars.next() {
      None => return Err(ParseError("re.error: bad escape (end of pattern)")),
      Some('\\') => expanded.push('\\'),
      Some('n') => expanded.push('\n'),
      Some('t') => expanded.push('\t'),
      Some('r') => expanded.push('\r'),
      Some('f') => expanded.push('\u{0c}'),
      Some('v') => expanded.push('\u{0b}'),
      Some('a') => expanded.push('\u{07}'),
      Some('b') => expanded.push('\u{08}'),
      Some(c) if c.is_ascii_alphanumeric() => return Err(ParseError("re.error: bad escape or group reference")),
      Some(c) => {
        expanded.push('\\');
        expanded.push(c);
      }
    }
  }
  Ok(family_replacement.replace("$1", &expanded))
}

fn non_empty(s: Option<&str>) -> Option<String> {
  s.filter(|s| !s.is_empty()).map(str::to_string)
}

/// Up to four version parts (major, minor, patch, patch_minor).
type Versions = [Option<String>; 4];

/// `_ParseUserAgent`: (family, [v1, v2, v3, None]).
fn parse_user_agent(ua: &str) -> Result<(String, Versions), ParseError> {
  // Like upstream's loop, the last parser's values are kept when none gives a family.
  let (mut family, mut v1, mut v2, mut v3) = (None, None, None, None);
  for (pattern, rule) in &PARSERS.user_agent {
    (family, v1, v2, v3) = (None, None, None, None);
    if let Some(m) = Match::search(pattern, ua) {
      family = match rule.family_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) if r.contains("$1") => Some(family_template(r, m.group(1))?),
        Some(r) => Some(r.to_string()),
        None => m.group(1).map(str::to_string),
      };
      v1 = match rule.v1_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => Some(r.to_string()),
        None if m.lastindex_at_least(2) => non_empty(m.group(2)),
        None => None,
      };
      v2 = match rule.v2_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => Some(r.to_string()),
        None if m.lastindex_at_least(3) => non_empty(m.group(3)),
        None => None,
      };
      if m.lastindex_at_least(4) {
        v3 = non_empty(m.group(4));
      }
    }
    if family.as_deref().is_some_and(|f| !f.is_empty()) {
      break;
    }
  }
  let family = family.filter(|f| !f.is_empty()).unwrap_or_else(|| "Other".into());
  Ok((
    family,
    [
      non_empty(v1.as_deref()),
      non_empty(v2.as_deref()),
      non_empty(v3.as_deref()),
      None,
    ],
  ))
}

/// `_ParseOS`: (family, [v1, v2, v3, v4]); unlike the browser's, empty versions are kept.
fn parse_os(ua: &str) -> (String, Versions) {
  let mut os = None;
  let mut versions: Versions = Default::default();
  for (pattern, rule) in &PARSERS.os {
    os = None;
    versions = Default::default();
    if let Some(m) = Match::search(pattern, ua) {
      os = match rule.os_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => multi_replace(r, &m),
        None if m.lastindex.is_some() => m.group(1).map(str::to_string),
        None => None,
      };
      let replacements = [
        &rule.os_v1_replacement,
        &rule.os_v2_replacement,
        &rule.os_v3_replacement,
        &rule.os_v4_replacement,
      ];
      for (i, replacement) in replacements.into_iter().enumerate() {
        versions[i] = match replacement.as_deref().filter(|r| !r.is_empty()) {
          Some(r) => multi_replace(r, &m),
          None if m.lastindex_at_least(i + 2) => m.group(i + 2).map(str::to_string),
          None => None,
        };
      }
    }
    if os.as_deref().is_some_and(|o| !o.is_empty()) {
      break;
    }
  }
  (os.filter(|o| !o.is_empty()).unwrap_or_else(|| "Other".into()), versions)
}

/// `_ParseDevice`: (family, brand, model).
fn parse_device(ua: &str) -> (String, Option<String>, Option<String>) {
  let (mut device, mut brand, mut model) = (None, None, None);
  for (pattern, rule) in &PARSERS.device {
    (device, brand, model) = (None, None, None);
    if let Some(m) = Match::search(pattern, ua) {
      device = match rule.device_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => multi_replace(r, &m),
        None => m.group(1).map(str::to_string),
      };
      brand = rule
        .brand_replacement
        .as_deref()
        .filter(|r| !r.is_empty())
        .and_then(|r| multi_replace(r, &m));
      model = match rule.model_replacement.as_deref().filter(|r| !r.is_empty()) {
        Some(r) => multi_replace(r, &m),
        None if m.group_count() > 0 => m.group(1).map(str::to_string),
        None => None,
      };
    }
    if device.as_deref().is_some_and(|d| !d.is_empty()) {
      break;
    }
  }
  // Only None becomes "Other" here (an empty family from the last parser stays empty).
  (device.unwrap_or_else(|| "Other".into()), brand, model)
}

/// `user_agents.parsers.parse_version` + `version_string`: up to three parts, digit-only ones as integers.
fn version_string(parts: &[Option<String>]) -> Result<String, ParseError> {
  let mut out = Vec::new();
  for part in parts.iter().take(3).flatten() {
    out.push(if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) {
      let trimmed = part.trim_start_matches('0');
      if trimmed.is_empty() {
        "0".to_string()
      } else {
        trimmed.to_string()
      }
    } else if part.chars().any(char::is_numeric) && part.chars().all(char::is_numeric) {
      // Python's str.isdigit() is true for e.g. "²", on which int() fails.
      return Err(ParseError("ValueError: invalid literal for int()"));
    } else {
      part.clone()
    });
  }
  Ok(out.join("."))
}

/// `get_contexts_enriched_with_ua`: the event's contexts, with `browser`, `os` and `device` derived from the request's
/// User-Agent header where the SDK didn't send them. If that fails, the contexts as they are.
pub fn contexts_enriched_with_user_agent(data: &Value) -> Value {
  let mut contexts = data
    .get("contexts")
    .filter(|c| c.is_object())
    .cloned()
    .unwrap_or_else(|| json!({}));
  let headers = data
    .get("request")
    .filter(|r| r.is_object())
    .and_then(|r| r.get("headers"))
    .filter(|h| h.is_object());
  let ua = match headers.and_then(|h| h.get("User-Agent")) {
    None | Some(Value::Null) => return contexts,
    Some(Value::Array(values)) => {
      match values.first() {
        None => return contexts,
        Some(first) => first, // assumed to be the only one
      }
    }
    Some(ua) => ua,
  };
  let parsed = match ua
    .as_str()
    .ok_or("TypeError: user agent is not a string")
    .and_then(|ua| parse(ua).map_err(|e| e.0))
  {
    Ok(parsed) => parsed,
    Err(error) => {
      tracing::warn!(user_agent = %ua, error, "failed to enrich contexts with User-Agent info");
      return contexts;
    }
  };
  let contexts_map = contexts.as_object_mut().expect("object");
  contexts_map
    .entry("browser")
    .or_insert_with(|| json!({"name": parsed.browser_family, "version": parsed.browser_version}));
  contexts_map
    .entry("os")
    .or_insert_with(|| json!({"name": parsed.os_family, "version": parsed.os_version}));
  contexts_map.entry("device").or_insert_with(
    || json!({"family": parsed.device_family, "model": parsed.device_model, "brand": parsed.device_brand}),
  );
  contexts
}

/// `user_agents.parse(ua)`, with upstream's (and `ua-parser`'s) 200-entry cache.
fn parse(ua: &str) -> Result<UserAgent, ParseError> {
  static CACHE: LazyLock<Mutex<HashMap<String, Result<UserAgent, &'static str>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
  if let Some(hit) = CACHE.lock().expect("cache").get(ua) {
    return hit.clone().map_err(ParseError);
  }
  let result = parse_uncached(ua);
  let mut cache = CACHE.lock().expect("cache");
  if cache.len() >= 200 {
    cache.clear();
  }
  cache.insert(ua.to_string(), result.as_ref().map(Clone::clone).map_err(|e| e.0));
  result
}

fn parse_uncached(ua: &str) -> Result<UserAgent, ParseError> {
  let (browser_family, browser_versions) = parse_user_agent(ua)?;
  let (os_family, os_versions) = parse_os(ua);
  let (device_family, device_brand, device_model) = parse_device(ua);
  Ok(UserAgent {
    browser_family,
    browser_version: version_string(&browser_versions)?,
    os_family,
    os_version: version_string(&os_versions)?,
    device_family,
    device_brand,
    device_model,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn all_regexes_compile() {
    let p = &*PARSERS;
    assert_eq!((p.user_agent.len(), p.os.len(), p.device.len()), (433, 204, 633));
  }

  #[test]
  fn lastindex_like_python() {
    let lastindex = |pattern: &str, s: &str| Match::search(&Pattern::new(pattern, false), s).unwrap().lastindex;
    assert_eq!(lastindex(r"((a)b)", "ab"), Some(1));
    assert_eq!(lastindex(r"(a)()", "a"), Some(2));
    assert_eq!(lastindex(r"a(b)?", "a"), None);
    assert_eq!(lastindex(r"(a)(?:x|(b))?([c)])", "ac"), Some(3));
  }

  #[test]
  fn templates_like_python() {
    assert_eq!(family_template("X $1", Some(r"a\b")).unwrap(), "X a\u{8}");
    assert!(family_template("X $1", Some(r"a\d")).is_err());
    assert!(family_template("X $1", None).is_err());
    assert_eq!(family_template("X $1 $1", Some(r"a\.")).unwrap(), r"X a\. a\.");
  }

  #[test]
  fn versions() {
    let v = |parts: &[&str]| version_string(&parts.iter().map(|p| Some(p.to_string())).collect::<Vec<_>>());
    assert_eq!(v(&["120", "0", "05", "9"]).unwrap(), "120.0.5");
    assert_eq!(v(&["10", "", "x1"]).unwrap(), "10..x1");
    assert!(v(&["²"]).is_err());
  }
}
