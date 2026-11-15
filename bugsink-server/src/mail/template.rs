//! Renders upstream's Django email templates (kept verbatim in `templates/`, except that the HTML ones share
//! `_layout.html`), supporting exactly the constructs they use: `{# comment #}`, `{{ var }}`,
//! `{{ var|truncatechars:N }}` and `{% if var %}...{% endif %}`. Like Django, every variable is HTML-escaped, in the
//! `.txt` template too.

use std::collections::HashMap;

use crate::py::{
  html_escape,
  truncatechars,
};

pub(crate) type Context<'a> = HashMap<&'static str, &'a str>;

/// Upstream's HTML emails are the same but for the middle part, which each email's own file holds.
const LAYOUT: &str = include_str!("templates/_layout.html");

/// An HTML email's template: its own part in the shared layout.
pub(crate) fn with_layout(content: &str) -> String {
  LAYOUT.replacen("@content@\n", content, 1)
}

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
    let end = tail
      .find(close)
      .unwrap_or_else(|| panic!("unterminated tag in template: {}", &tail[..20]));
    let inner = tail[2..end].trim();
    rest = &tail[end + 2..];
    match &tail[..2] {
      "{{" => emit(&mut out, &conditions, "", &html_escape(&variable(inner, ctx))),
      "{%" => {
        match inner.split_whitespace().collect::<Vec<_>>().as_slice() {
          ["if", name] => conditions.push(!lookup(name, ctx).is_empty()),
          ["endif"] => {
            conditions.pop().expect("endif without if");
          }
          _ => panic!("unsupported template tag: {inner}"),
        }
      }
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
    Some((name, filter)) => {
      match filter.trim().split_once(':') {
        Some(("truncatechars", n)) => truncatechars(lookup(name.trim(), ctx), n.parse().expect("truncatechars length")),
        _ => panic!("unsupported template filter: {filter}"),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn subset() {
    let ctx: Context = [("a", "x & 'y'"), ("t", "abcdef"), ("e", "")].into_iter().collect();
    let t =
      "{# c #}[{{ a }}] {{ t|truncatechars:4 }}\n{% if e %}no{% endif %}{% if t %}yes {{ missing }}{% endif %} {x}";
    assert_eq!(render(t, &ctx), "[x &amp; &#x27;y&#x27;] abc…\nyes  {x}");
  }

  #[test]
  fn layout_gives_upstreams_templates() {
    use sha2::{
      Digest,
      Sha256,
    };
    for (content, sha256) in [
      (
        include_str!("templates/confirm_email.html"),
        "026225ff9234139f45b323399d557e2eb1a013ad3c2f8de91c92cbd6398c41be",
      ),
      (
        include_str!("templates/issue_alert.html"),
        "0b41ca26238cb801a0a00fbcc81b4f5e7b1c9e9b486592b29ef817725eb731bc",
      ),
      (
        include_str!("templates/project_membership_invite.html"),
        "0152575c407bcc53bcc6ce91294a13fc42e6fc1f7e90d01e2b59e71aab674218",
      ),
      (
        include_str!("templates/project_membership_invite_new_user.html"),
        "f6037586f931e2d84b2b1716a05071359e62549826bba5bddf598bff4c523417",
      ),
      (
        include_str!("templates/reset_password_email.html"),
        "d56885904e3a78bb0fd190fb3a05eb6acbf1dc1a703322a2604401381630fba7",
      ),
      (
        include_str!("templates/team_membership_invite.html"),
        "27cda11fb85a133254c3b6778fcc22651725c992f253dfb0237d77def69c80c1",
      ),
      (
        include_str!("templates/team_membership_invite_new_user.html"),
        "a378bf20cc12f002451d7454803d6a8a932f1188a0b48350782bdd1fc30a78a7",
      ),
      (
        include_str!("templates/welcome_email.html"),
        "73964b39d1532b2c5ef8a3a312993007ae19ea503bb37dc5d9e17c78c5753adb",
      ),
    ] {
      assert_eq!(format!("{:x}", Sha256::digest(with_layout(content))), sha256);
    }
  }
}
