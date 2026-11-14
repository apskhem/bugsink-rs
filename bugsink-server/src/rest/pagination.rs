//! Django REST Framework's `CursorPagination`, which upstream's lists use: a cursor is a position (the first ordering
//! field's value, as Python's `str()`), an offset among rows sharing that position, and a direction. Ported as is,
//! including how the `next`/`previous` links are built, so that cursors and links look like upstream's.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bugsink_db::{PgPool, Postgres};
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use sqlx::postgres::PgRow;

use super::fields::{PyInt, py_int};
use super::{Drf, Request, quote_plus, url_parse_qsl};

/// `offset_cutoff`.
const OFFSET_CUTOFF: i64 = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    pub offset: i64,
    pub reverse: bool,
    pub position: Option<String>,
}

/// How a list is ordered: SQL expressions with their direction (the first is the cursor's position), and the SQL
/// type to compare a position with.
pub struct Ordering {
    pub fields: Vec<(&'static str, bool)>,
    pub cast: &'static str,
    pub page_size: i64,
}

impl Ordering {
    /// Upstream's `AscDescCursorPagination`: `?order=asc|desc`, else the default.
    pub fn asc_desc(
        req: &Request,
        fields: &[&'static str],
        cast: &'static str,
        default_desc: bool,
    ) -> Result<Self, Drf> {
        let desc = match req.query.get("order") {
            None | Some("") => default_desc,
            Some("asc") => false,
            Some("desc") => true,
            Some(_) => return Err(Drf::invalid(json!({"order": ["Must be 'asc' or 'desc'."]}))),
        };
        Ok(Self { fields: fields.iter().map(|f| (*f, desc)).collect(), cast, page_size: 250 })
    }
}

/// A page and the links around it: `{"next", "previous", "results"}`.
pub struct Page<T> {
    pub items: Vec<T>,
    next: Option<Cursor>,
    previous: Option<Cursor>,
}

impl<T> Page<T> {
    pub fn response(&self, req: &Request, results: Vec<Value>) -> Value {
        let link = |c: &Option<Cursor>| c.as_ref().map_or(Value::Null, |c| Value::String(link(req, c)));
        json!({"next": link(&self.next), "previous": link(&self.previous), "results": results})
    }
}

/// `decode_cursor`: none asked for, the one asked for, or "Invalid cursor".
pub fn decode_cursor(req: &Request) -> Result<Option<Cursor>, Drf> {
    let Some(encoded) = req.query.get("cursor") else { return Ok(None) };
    let invalid = || Drf::detail(axum::http::StatusCode::NOT_FOUND, "Invalid cursor");
    let bytes = b64decode(encoded).ok_or_else(invalid)?;
    let querystring = String::from_utf8(bytes).ok().filter(|s| s.is_ascii()).ok_or_else(invalid)?;
    let tokens = url_parse_qsl(&querystring);
    let first = |k: &str| tokens.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let offset = match py_int(first("o").unwrap_or("0")) {
        Some(PyInt::Small(n)) if n >= 0 => n.min(OFFSET_CUTOFF),
        Some(PyInt::Big(false)) => OFFSET_CUTOFF,
        _ => return Err(invalid()),
    };
    let reverse = match py_int(first("r").unwrap_or("0")) {
        Some(PyInt::Small(n)) => n != 0,
        Some(PyInt::Big(_)) => true,
        None => return Err(invalid()),
    };
    Ok(Some(Cursor { offset, reverse, position: first("p").map(str::to_owned) }))
}

/// Python's lenient `b64decode` (binascii's non-strict mode): characters outside the alphabet are skipped, and
/// decoding stops at padding that completes a quad; otherwise the quads must be complete.
fn b64decode(s: &str) -> Option<Vec<u8>> {
    if !s.is_ascii() {
        return None;
    }
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let (mut out, mut quad, mut left, mut pads) = (Vec::new(), 0, 0u32, 0);
    for c in s.bytes() {
        if c == b'=' {
            if quad >= 2 {
                pads += 1;
                if quad + pads >= 4 {
                    return Some(out);
                }
            }
            continue;
        }
        let Some(v) = value(c) else { continue };
        pads = 0;
        match quad {
            0 => left = v,
            1 => {
                out.push(((left << 2) | (v >> 4)) as u8);
                left = v & 0x0f;
            }
            2 => {
                out.push(((left << 4) | (v >> 2)) as u8);
                left = v & 0x03;
            }
            _ => out.push(((left << 6) | v) as u8),
        }
        quad = (quad + 1) % 4;
    }
    (quad == 0).then_some(out)
}

/// `paginate_queryset`: `query` selects the rows (ending in a WHERE clause); `position` is a row's cursor position.
pub async fn paginate<T>(
    pool: &PgPool,
    mut query: QueryBuilder<Postgres>,
    ordering: &Ordering,
    cursor: Option<Cursor>,
    position: impl Fn(&T) -> String,
) -> Result<Page<T>, Drf>
where
    T: for<'r> sqlx::FromRow<'r, PgRow> + Send + Unpin,
{
    let (offset, reverse, current) = match &cursor {
        None => (0, false, None),
        Some(c) => (c.offset, c.reverse, c.position.clone()),
    };
    let (first, first_desc) = ordering.fields[0];
    if let Some(position) = &current {
        // `__lt` when going against the first field's direction, else `__gt`
        let op = if reverse != first_desc { " < " } else { " > " };
        query.push(format!(" AND {first}{op}"));
        query.push_bind(position.clone());
        query.push(format!("::{}", ordering.cast));
    }
    let order: Vec<String> = ordering
        .fields
        .iter()
        .map(|(f, desc)| format!("{f} {}", if desc ^ reverse { "DESC" } else { "ASC" }))
        .collect();
    query.push(format!(" ORDER BY {} OFFSET {offset} LIMIT {}", order.join(", "), ordering.page_size + 1));
    let mut results: Vec<T> = query.build_query_as().fetch_all(pool).await?;

    let following = (results.len() as i64 > ordering.page_size).then(|| position(results.last().expect("non-empty")));
    results.truncate(ordering.page_size as usize);
    let mut page = results;
    let (has_next, has_previous, next_position, previous_position);
    if reverse {
        page.reverse();
        has_next = current.is_some() || offset > 0;
        has_previous = following.is_some();
        next_position = current.clone();
        previous_position = following;
    } else {
        has_next = following.is_some();
        has_previous = current.is_some() || offset > 0;
        next_position = following;
        previous_position = current.clone();
    }
    let positions: Vec<String> = page.iter().map(&position).collect();
    let page_size = ordering.page_size;

    // `get_next_link`
    let next = has_next.then(|| {
        let mut compare = match &cursor {
            Some(c) if !positions.is_empty() && c.reverse && c.offset != 0 => positions.last().cloned(),
            _ => next_position.clone(),
        };
        let mut offset = 0;
        let mut found = None;
        for p in positions.iter().rev() {
            if Some(p) != compare.as_ref() {
                found = Some(p.clone());
                break;
            }
            compare = Some(p.clone());
            offset += 1;
        }
        let mut position = found.clone();
        if !positions.is_empty() && found.is_none() {
            if !has_previous {
                offset = page_size;
                position = None;
            } else if cursor.as_ref().is_some_and(|c| c.reverse) {
                offset = 0;
                position = previous_position.clone();
            } else {
                offset = cursor.as_ref().map_or(0, |c| c.offset) + page_size;
                position = previous_position.clone();
            }
        }
        if positions.is_empty() {
            position = next_position.clone();
        }
        Cursor { offset, reverse: false, position }
    });

    // `get_previous_link`
    let previous = has_previous.then(|| {
        let mut compare = match &cursor {
            Some(c) if !positions.is_empty() && !c.reverse && c.offset != 0 => positions.first().cloned(),
            _ => previous_position.clone(),
        };
        let mut offset = 0;
        let mut found = None;
        for p in &positions {
            if Some(p) != compare.as_ref() {
                found = Some(p.clone());
                break;
            }
            compare = Some(p.clone());
            offset += 1;
        }
        let mut position = found.clone();
        if !positions.is_empty() && found.is_none() {
            if !has_next {
                offset = page_size;
                position = None;
            } else if cursor.as_ref().is_some_and(|c| c.reverse) {
                offset = cursor.as_ref().map_or(0, |c| c.offset) + page_size;
                position = next_position.clone();
            } else {
                offset = 0;
                position = next_position.clone();
            }
        }
        if positions.is_empty() {
            position = previous_position.clone();
        }
        Cursor { offset, reverse: true, position }
    });

    Ok(Page { items: page, next, previous })
}

/// `encode_cursor` + `replace_query_param` on the request's absolute URI.
fn link(req: &Request, cursor: &Cursor) -> String {
    let mut tokens = Vec::new();
    if cursor.offset != 0 {
        tokens.push(("o", cursor.offset.to_string()));
    }
    if cursor.reverse {
        tokens.push(("r", "1".into()));
    }
    if let Some(p) = &cursor.position {
        tokens.push(("p", p.clone()));
    }
    let querystring: Vec<String> = tokens.iter().map(|(k, v)| format!("{k}={}", quote_plus(v))).collect();
    let encoded = STANDARD.encode(querystring.join("&"));

    // `parse_qs` keeps keys in order of appearance, with all their values; then sorted by key
    let mut params: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in url_parse_qsl(&req.raw_query) {
        match params.iter_mut().find(|(key, _)| *key == k) {
            Some((_, values)) => values.push(v),
            None => params.push((k, vec![v])),
        }
    }
    match params.iter_mut().find(|(key, _)| key == "cursor") {
        Some((_, values)) => *values = vec![encoded],
        None => params.push(("cursor".into(), vec![encoded])),
    }
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let query: Vec<String> = params
        .iter()
        .flat_map(|(k, values)| values.iter().map(move |v| format!("{}={}", quote_plus(k), quote_plus(v))))
        .collect();
    format!("{}?{}", req.absolute_path(), query.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_like_python() {
        assert_eq!(b64decode("bz0yNTA="), Some(b"o=250".to_vec()));
        assert_eq!(b64decode("bz0yNTA%3D".replace("%3D", "=").as_str()), Some(b"o=250".to_vec()));
        assert_eq!(b64decode("bz0xMA"), None); // incorrect padding
        assert_eq!(b64decode("bz0xMA=="), Some(b"o=10".to_vec()));
        assert_eq!(b64decode("b z0x\nMA=="), Some(b"o=10".to_vec()));
        assert_eq!(b64decode(""), Some(vec![]));
    }
}
