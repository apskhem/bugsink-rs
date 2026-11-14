//! Searching issues and events: a port of upstream Bugsink 2.6.3's `tags/search.py`. A query is `key:value` tag terms
//! (quotes group spaces and colons, `\"` and `\\` escape) plus plain text; all must match. Tags match exactly against
//! the project's tag values; plain text matches the type or value case-insensitively anywhere.
//!
//! One deliberate difference: a query that is only whitespace (or only skipped terms) matches everything. Upstream
//! fails on it with a 500.

use bugsink_db::{PgPool, Postgres};
use sqlx::QueryBuilder;
use uuid::Uuid;

use crate::py::{is_space, strip};

/// `ParsedQuery`: tags in first-seen key order (a repeated key keeps its first position, with its last value).
#[derive(Debug, PartialEq)]
pub struct ParsedQuery {
    pub tags: Vec<(String, String)>,
    pub plain_text: String,
}

/// `parse_query`.
pub fn parse_query(q: &str) -> ParsedQuery {
    let chars: Vec<char> = q.chars().collect();
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut slices_to_remove = Vec::new();
    for (start, stop, quotes_closed) in split_on_spaces_respecting_quotes(&chars) {
        if !quotes_closed {
            continue;
        }
        let Some((key, value)) = parse_tag_token(&chars[start..stop]) else { continue };
        if key.is_empty() {
            continue;
        }
        match tags.iter_mut().find(|(k, _)| *k == key) {
            Some(existing) => existing.1 = value,
            None => tags.push((key, value)),
        }
        slices_to_remove.push((start, stop));
    }

    let mut kept = String::new();
    let mut at = 0;
    for (start, stop) in slices_to_remove {
        kept.extend(&chars[at..start]);
        at = stop;
    }
    kept.extend(&chars[at..]);
    ParsedQuery { tags, plain_text: strip(&kept).to_owned() }
}

/// `_split_on_spaces_respecting_quotes`: (start, stop, quotes closed) per term, in chars.
fn split_on_spaces_respecting_quotes(q: &[char]) -> Vec<(usize, usize, bool)> {
    let mut terms = Vec::new();
    let mut start = None;
    let mut in_quotes = false;
    let mut i = 0;
    while i < q.len() {
        if start.is_none() {
            if is_space(q[i]) {
                i += 1;
                continue;
            }
            start = Some(i);
        }
        if is_escape(q, i) {
            i += 2;
            continue;
        }
        if q[i] == '"' {
            in_quotes = !in_quotes;
        } else if is_space(q[i]) && !in_quotes {
            terms.push((start.take().unwrap(), i, true));
        }
        i += 1;
    }
    if let Some(start) = start {
        terms.push((start, q.len(), !in_quotes));
    }
    terms
}

/// `\"` or `\\` at `i`.
fn is_escape(s: &[char], i: usize) -> bool {
    s[i] == '\\' && s.get(i + 1).is_some_and(|c| matches!(c, '"' | '\\'))
}

/// `_parse_tag_token`: split on the first colon outside quotes, unless it is the first or last character.
fn parse_tag_token(term: &[char]) -> Option<(String, String)> {
    let mut in_quotes = false;
    let mut i = 0;
    while i < term.len() {
        if is_escape(term, i) {
            i += 2;
            continue;
        }
        if term[i] == '"' {
            in_quotes = !in_quotes;
        } else if term[i] == ':' && !in_quotes {
            if i == 0 || i == term.len() - 1 {
                return None;
            }
            return Some((unquote(&term[..i]), unquote(&term[i + 1..])));
        }
        i += 1;
    }
    None
}

/// `_unquote`: drop quotes, resolve escapes.
fn unquote(value: &[char]) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i < value.len() {
        if is_escape(value, i) {
            result.push(value[i + 1]);
            i += 2;
        } else {
            if value[i] != '"' {
                result.push(value[i]);
            }
            i += 1;
        }
    }
    result
}

/// A query resolved against a project's tags, ready to filter a list (`_search`).
#[derive(Debug, PartialEq)]
pub enum Search {
    Everything,
    /// A tag value that doesn't exist in the project: nothing can match.
    Nothing,
    Matching {
        /// One per tag term; all must be on the issue (or event).
        tag_value_ids: Vec<i64>,
        /// `icontains` pattern for plain text.
        like: Option<String>,
    },
}

/// Which list is being searched: issues (via `tags_issuetag`) or one issue's events (via `tags_eventtag`).
#[derive(Clone, Copy)]
pub enum Searched {
    Issues,
    EventsOf(Uuid),
}

impl Search {
    pub async fn prepare(pool: &PgPool, project_id: i64, q: &str) -> sqlx::Result<Self> {
        if q.is_empty() {
            return Ok(Self::Everything);
        }
        let parsed = parse_query(q);
        let mut tag_value_ids = Vec::with_capacity(parsed.tags.len());
        for (key, value) in &parsed.tags {
            let id: Option<i64> = sqlx::query_scalar(
                "SELECT v.id FROM tags_tagvalue v JOIN tags_tagkey k ON k.id = v.key_id
                 WHERE k.project_id = $1 AND k.key = $2 AND v.value = $3",
            )
            .bind(project_id)
            .bind(key)
            .bind(value)
            .fetch_optional(pool)
            .await?;
            match id {
                Some(id) => tag_value_ids.push(id),
                None => return Ok(Self::Nothing),
            }
        }
        let like = (!parsed.plain_text.is_empty()).then(|| contains_pattern(&parsed.plain_text));
        if tag_value_ids.is_empty() && like.is_none() {
            return Ok(Self::Everything);
        }
        Ok(Self::Matching { tag_value_ids, like })
    }

    /// Appends ` AND ...` conditions on the searched table's `id`, `calculated_type` and `calculated_value`.
    pub fn push_conditions(&self, qb: &mut QueryBuilder<Postgres>, searched: Searched) {
        let Self::Matching { tag_value_ids, like } = self else {
            if *self == Self::Nothing {
                qb.push(" AND false");
            }
            return;
        };
        if !tag_value_ids.is_empty() {
            // every tag: as many matching tag rows as tags (a value is on an issue or event at most once)
            match searched {
                Searched::Issues => {
                    qb.push(" AND id IN (SELECT issue_id FROM tags_issuetag WHERE value_id = ANY(");
                    qb.push_bind(tag_value_ids.clone());
                    qb.push(") GROUP BY issue_id HAVING COUNT(*) = ");
                }
                Searched::EventsOf(issue_id) => {
                    qb.push(" AND id IN (SELECT event_id FROM tags_eventtag WHERE issue_id = ");
                    qb.push_bind(issue_id);
                    qb.push(" AND value_id = ANY(");
                    qb.push_bind(tag_value_ids.clone());
                    qb.push(") GROUP BY event_id HAVING COUNT(*) = ");
                }
            }
            qb.push(tag_value_ids.len() as i64);
            qb.push(")");
        }
        if let Some(like) = like {
            push_icontains(qb, "", like);
        }
    }
}

/// The `LIKE` pattern for `icontains`: the text, escaped as Django's `prep_for_like_query` does, anywhere.
pub(crate) fn contains_pattern(text: &str) -> String {
    format!("%{}%", escape_like(text))
}

/// Django's `icontains` on Postgres, for the issue's type or value (`prefix`: the issue table's alias and a dot).
pub(crate) fn push_icontains(qb: &mut QueryBuilder<Postgres>, prefix: &'static str, like: &str) {
    qb.push(format!(" AND (UPPER({prefix}calculated_type::text) LIKE UPPER("));
    qb.push_bind(like.to_owned());
    qb.push(format!(") OR UPPER({prefix}calculated_value::text) LIKE UPPER("));
    qb.push_bind(like.to_owned());
    qb.push("))");
}

/// Django's `prep_for_like_query`.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(tags: &[(&str, &str)], plain_text: &str) -> ParsedQuery {
        ParsedQuery {
            tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            plain_text: plain_text.into(),
        }
    }

    #[test]
    fn parses_like_upstream() {
        assert_eq!(parse_query("region:eu disk full"), parsed(&[("region", "eu")], "disk full"));
        assert_eq!(parse_query(r#"msg:"a b:c" x"#), parsed(&[("msg", "a b:c")], "x"));
        assert_eq!(parse_query(r#""a:b":c"#), parsed(&[("a:b", "c")], ""));
        assert_eq!(parse_query(r#"k:"say \"hi\"""#), parsed(&[("k", r#"say "hi""#)], ""));
        assert_eq!(parse_query("a: :b a:b:c"), parsed(&[("a", "b:c")], "a: :b"));
        assert_eq!(parse_query(r#"k:"unclosed rest"#), parsed(&[], r#"k:"unclosed rest"#));
        assert_eq!(parse_query("k:1 j:2 k:3"), parsed(&[("k", "3"), ("j", "2")], ""));
        assert_eq!(parse_query("\"\":x"), parsed(&[], "\"\":x"));
        assert_eq!(parse_query("é:ü\u{a0}text\u{1c}"), parsed(&[("é", "ü")], "text"));
    }

    #[test]
    fn escapes_like_django() {
        assert_eq!(escape_like(r"a%_\b"), r"a\%\_\\b");
    }
}
