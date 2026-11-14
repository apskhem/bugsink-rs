//! Renders upstream's Django email templates (kept verbatim in `templates/`), supporting exactly the constructs they
//! use: `{# comment #}`, `{{ var }}`, `{{ var|truncatechars:N }}` and `{% if var %}...{% endif %}`. Like Django,
//! every variable is HTML-escaped, in the `.txt` template too.

use std::collections::HashMap;

use crate::grouping::py::truncatechars;

pub(crate) type Context<'a> = HashMap<&'static str, &'a str>;

pub(crate) fn render(template: &str, ctx: &Context) -> String {
    let mut out = String::new();
    // Nesting depth of `{% if %}` blocks, and how many of them are false (output suppressed while > 0).
    let mut conditions: Vec<bool> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let (text, tail) = rest.split_at(start);
        let close = match tail.as_bytes().get(1) {
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            Some(b'#') => "#}",
            _ => {
                emit(&mut out, &conditions, &tail[..1], text);
                rest = &tail[1..];
                continue;
            }
        };
        emit(&mut out, &conditions, "", text);
        let end = tail.find(close).unwrap_or_else(|| panic!("unterminated tag in template: {}", &tail[..20]));
        let inner = tail[2..end].trim();
        rest = &tail[end + 2..];
        match &tail[..2] {
            "{{" => emit(&mut out, &conditions, "", &escape(&variable(inner, ctx))),
            "{%" => match inner.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["if", name] => conditions.push(!lookup(name, ctx).is_empty()),
                ["endif"] => {
                    conditions.pop().expect("endif without if");
                }
                _ => panic!("unsupported template tag: {inner}"),
            },
            _ => {} // comment
        }
    }
    emit(&mut out, &conditions, "", rest);
    out
}

fn emit(out: &mut String, conditions: &[bool], prefix: &str, text: &str) {
    if conditions.iter().all(|c| *c) {
        out.push_str(text);
        out.push_str(prefix);
    }
}

fn lookup<'a>(name: &str, ctx: &Context<'a>) -> &'a str {
    ctx.get(name).copied().unwrap_or("")
}

fn variable(expr: &str, ctx: &Context) -> String {
    match expr.split_once('|') {
        None => lookup(expr.trim(), ctx).to_string(),
        Some((name, filter)) => match filter.trim().split_once(':') {
            Some(("truncatechars", n)) => {
                truncatechars(lookup(name.trim(), ctx), n.parse().expect("truncatechars length"))
            }
            _ => panic!("unsupported template filter: {filter}"),
        },
    }
}

/// Django's `escape`.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset() {
        let ctx: Context = [("a", "x & 'y'"), ("t", "abcdef"), ("e", "")].into_iter().collect();
        let t = "{# c #}[{{ a }}] {{ t|truncatechars:4 }}\n{% if e %}no{% endif %}{% if t %}yes {{ missing }}{% endif %} {x}";
        assert_eq!(render(t, &ctx), "[x &amp; &#x27;y&#x27;] abc…\nyes  {x}");
    }
}
