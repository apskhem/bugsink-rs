//! Django's validation of what users type, as upstream's forms apply it: email addresses (`EmailField`) and new
//! passwords (`AUTH_PASSWORD_VALIDATORS`: similarity to the user's attributes, minimum length, common passwords, all
//! digits; every failing validator's message).

use std::collections::HashSet;
use std::io::Read;
use std::sync::OnceLock;

use regex::Regex;

use crate::grouping::py::{is_space, strip};

/// `forms.EmailField`: strips the value, then `validate_email`. The cleaned address, or Django's message.
pub fn clean_email(value: &str) -> Result<String, &'static str> {
    const INVALID: &str = "Enter a valid email address.";
    let value = strip(value);
    if value.is_empty() {
        return Err("This field is required.");
    }
    if value.chars().count() > 320 {
        return Err(INVALID);
    }
    let Some((user, domain)) = value.rsplit_once('@') else { return Err(INVALID) };
    static USER: OnceLock<Regex> = OnceLock::new();
    static DOMAIN: OnceLock<Regex> = OnceLock::new();
    static LITERAL: OnceLock<Regex> = OnceLock::new();
    let user_re = USER.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)(^[-!#$%&'*+/=?^_`{}|~0-9A-Z]+(\.[-!#$%&'*+/=?^_`{}|~0-9A-Z]+)*\z",
            r#"|^"([\x01-\x08\x0b\x0c\x0e-\x1f!#-\[\]-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*"\z)"#,
        ))
        .unwrap()
    });
    if !user_re.is_match(user) {
        return Err(INVALID);
    }
    // DomainNameValidator's parts; the look-arounds are checked by hand below (the regex crate has none)
    let domain_re = DOMAIN.get_or_init(|| {
        let ul = "\u{a1}-\u{ffff}";
        Regex::new(&format!(
            concat!(
                r"(?i)^[a-z{ul}0-9](?:[a-z{ul}0-9-]{{0,61}}[a-z{ul}0-9])?",
                r"((?:\.[a-z{ul}0-9-]{{1,63}})*)\.((?:[a-z{ul}-]{{2,63}}|xn--[a-z0-9]{{1,59}}))\z",
            ),
            ul = ul
        ))
        .unwrap()
    });
    let domain_ok = domain == "localhost"
        || domain_re.captures(domain).is_some_and(|c| {
            // (?!-) and (?<!-) around every label after the first, and around the top-level one
            let labels = c[1].split('.').skip(1).chain(std::iter::once(&c[2]));
            labels.into_iter().all(|l| !l.starts_with('-') && !l.ends_with('-'))
        })
        || LITERAL
            .get_or_init(|| Regex::new(r"(?i)^\[([A-F0-9:.]+)\]\z").unwrap())
            .captures(domain)
            .is_some_and(|c| c[1].parse::<std::net::IpAddr>().is_ok());
    if !domain_ok {
        return Err(INVALID);
    }
    Ok(value.to_owned())
}

/// What the similarity validator compares a password with.
pub struct UserAttributes<'a> {
    pub username: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub email: &'a str,
}

/// `SetPasswordForm`'s new password: required, then `validate_password` with upstream's validators, in their order;
/// empty when the password is fine. (Python's `isdigit` is approximated by `char::is_numeric`.)
pub fn password_errors(password: &str, user: &UserAttributes) -> Vec<String> {
    if password.is_empty() {
        return vec!["This field is required.".into()];
    }
    let mut errors = Vec::new();
    if let Some(attribute) = too_similar(password, user) {
        errors.push(format!("The password is too similar to the {attribute}."));
    }
    if password.chars().count() < 8 {
        errors.push("This password is too short. It must contain at least 8 characters.".into());
    }
    if common_passwords().contains(strip(&password.to_lowercase())) {
        errors.push("This password is too common.".into());
    }
    if !password.is_empty() && password.chars().all(|c| c.is_numeric()) {
        errors.push("This password is entirely numeric.".into());
    }
    errors
}

/// `UserAttributeSimilarityValidator` (max similarity 0.7): the verbose name of the first attribute the password is
/// too similar to, comparing with the whole value and its parts between non-word characters.
fn too_similar(password: &str, user: &UserAttributes) -> Option<&'static str> {
    const MAX_SIMILARITY: f64 = 0.7;
    let password: Vec<char> = password.to_lowercase().chars().collect();
    let attributes = [
        ("username", user.username),
        ("first name", user.first_name),
        ("last name", user.last_name),
        ("email address", user.email),
    ];
    static NON_WORD: OnceLock<Regex> = OnceLock::new();
    let non_word = NON_WORD.get_or_init(|| Regex::new(r"\W+").unwrap());
    for (verbose_name, value) in attributes {
        if value.is_empty() {
            continue;
        }
        let value = value.to_lowercase();
        let mut parts: Vec<&str> = non_word.split(&value).collect();
        parts.push(&value);
        for part in parts {
            let part: Vec<char> = part.chars().collect();
            // `exceeds_maximum_length_ratio`
            let (pwd_len, value_len) = (password.len() as f64, part.len() as f64);
            if pwd_len >= 10.0 * value_len && value_len < MAX_SIMILARITY / 2.0 * pwd_len {
                continue;
            }
            if quick_ratio(&password, &part) >= MAX_SIMILARITY {
                return Some(verbose_name);
            }
        }
    }
    None
}

/// `difflib.SequenceMatcher(a=a, b=b).quick_ratio()`: twice the multiset intersection over the total length.
fn quick_ratio(a: &[char], b: &[char]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut available = std::collections::HashMap::new();
    for c in b {
        *available.entry(c).or_insert(0i64) += 1;
    }
    let mut matches = 0;
    for c in a {
        let n = available.entry(c).or_insert(0);
        if *n > 0 {
            matches += 1;
        }
        *n -= 1;
    }
    2.0 * matches as f64 / (a.len() + b.len()) as f64
}

/// Django's `common-passwords.txt.gz`, as shipped in upstream's image.
fn common_passwords() -> &'static HashSet<String> {
    static PASSWORDS: OnceLock<HashSet<String>> = OnceLock::new();
    PASSWORDS.get_or_init(|| {
        let mut text = String::new();
        flate2::read::GzDecoder::new(&include_bytes!("common-passwords.txt.gz")[..])
            .read_to_string(&mut text)
            .expect("bundled list");
        text.lines().map(|l| l.trim_matches(is_space).to_owned()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: UserAttributes = UserAttributes {
        username: "ada.lovelace@example.org",
        first_name: "Ada",
        last_name: "Lovelace",
        email: "ada.lovelace@example.org",
    };

    #[test]
    fn passwords_like_django() {
        assert!(password_errors("correct horse battery", &USER).is_empty());
        assert_eq!(
            password_errors("password", &USER),
            vec!["This password is too common.".to_string()]
        );
        assert_eq!(
            password_errors("1234", &USER),
            vec![
                "This password is too short. It must contain at least 8 characters.".to_string(),
                "This password is too common.".into(),
                "This password is entirely numeric.".into(),
            ]
        );
        let similar = vec!["The password is too similar to the username.".to_string()];
        assert_eq!(password_errors("lovelace1", &USER), similar);
    }

    #[test]
    fn emails_like_django() {
        assert_eq!(clean_email("  a@example.org "), Ok("a@example.org".into()));
        let valid =
            ["a.b+c@sub.example.co", "x@localhost", "\"q@u\"@example.org", "a@[127.0.0.1]", "a@xn--bcher-kva.ch"];
        for ok in valid {
            assert!(clean_email(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a", "a@b", "a@-b.com", "a@b-.com", "a@b.c", "a@b.-com", ".a@b.com", "a@[1.2.3]"] {
            assert!(clean_email(bad).is_err(), "{bad}");
        }
    }
}
