//! Django's default password hasher (`PBKDF2PasswordHasher`, Django 5.2 as in upstream's image), so that passwords set
//! by either side work on the other: `pbkdf2_sha256$<iterations>$<salt>$<base64 hash>` in `auth_user.password`.
//!
//! Other Django hashers (sha1, argon2, bcrypt, scrypt) are not configured by upstream and not supported: such a
//! password never matches.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::Sha256;
use subtle::ConstantTimeEq;

const ALGORITHM: &str = "pbkdf2_sha256";
const ITERATIONS: u32 = 1_000_000;
/// Django's `RANDOM_STRING_CHARS`.
const SALT_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// ceil(128 bits of `salt_entropy` / log2(62)).
const SALT_LEN: usize = 22;
/// `django.contrib.auth.hashers.UNUSABLE_PASSWORD_PREFIX`.
const UNUSABLE_PREFIX: char = '!';

#[derive(Debug, PartialEq, Eq)]
pub struct Verified {
    pub matches: bool,
    /// Django's `must_update`: on a successful login the password is rehashed with today's parameters.
    pub must_update: bool,
}

/// `check_password` without the setter; slow on purpose, so call it off the async runtime.
pub fn verify(password: &str, encoded: &str) -> Verified {
    let no = Verified { matches: false, must_update: false };
    if encoded.is_empty() || encoded.starts_with(UNUSABLE_PREFIX) {
        return no;
    }
    let parts: Vec<&str> = encoded.splitn(4, '$').collect();
    let &[algorithm, iterations, salt, _hash] = parts.as_slice() else { return no };
    if algorithm != ALGORITHM {
        tracing::warn!(algorithm, "password uses a hasher upstream does not configure; it cannot be checked");
        return no;
    }
    let Ok(iterations) = iterations.parse::<u32>() else { return no };
    if iterations == 0 {
        return no;
    }
    let matches: bool = encode(password, salt, iterations).as_bytes().ct_eq(encoded.as_bytes()).into();
    Verified { matches, must_update: matches && (iterations != ITERATIONS || salt.len() < SALT_LEN) }
}

/// `make_password` with a fresh salt.
pub fn make(password: &str) -> String {
    let mut rng = rand::rng();
    let salt: String = (0..SALT_LEN).map(|_| SALT_CHARS[rng.random_range(0..SALT_CHARS.len())] as char).collect();
    encode(password, &salt, ITERATIONS)
}

fn encode(password: &str, salt: &str, iterations: u32) -> String {
    let hash = STANDARD.encode(pbkdf2_sha256(password.as_bytes(), salt.as_bytes(), iterations));
    format!("{ALGORITHM}${iterations}${salt}${hash}")
}

/// PBKDF2-HMAC-SHA256 with a 32-byte output, i.e. exactly one block (RFC 8018).
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let keyed = Hmac::<Sha256>::new_from_slice(password).expect("HMAC takes keys of any length");
    let mut mac = keyed.clone();
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut out = u;
    for _ in 1..iterations {
        let mut mac = keyed.clone();
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        out.iter_mut().zip(u).for_each(|(o, x)| *o ^= x);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generated with upstream's image: PBKDF2PasswordHasher().encode(password, salt, iterations=...) and
    // make_password(password, salt).
    const DJANGO_1000: &str = "pbkdf2_sha256$1000$abcdefghijklmnopqrstuv$3sV/SVfT6ME4dIgYVdSlH0jEvyK0z+y3AJvov9Y+8MA=";
    const DJANGO_UNICODE: &str = "pbkdf2_sha256$1$Zx9$yypTxbQAbsI4Um3wCKsmXXUokVLS8Qz3rfJFk2qt7Rs=";
    const DJANGO_DEFAULT: &str =
        "pbkdf2_sha256$1000000$abcdefghijklmnopqrstuv$RQsDuUkUYsVKKMGEVf7KhP1xnDLlZ9RiOO63KN0aAbE=";

    #[test]
    fn encodes_like_django() {
        assert_eq!(encode("correct horse", "abcdefghijklmnopqrstuv", 1000), DJANGO_1000);
        assert_eq!(encode("pässwörd $ü", "Zx9", 1), DJANGO_UNICODE);
    }

    #[test]
    fn verifies_django_hashes() {
        assert_eq!(verify("correct horse", DJANGO_DEFAULT), Verified { matches: true, must_update: false });
        assert_eq!(verify("correct horse", DJANGO_1000), Verified { matches: true, must_update: true });
        assert_eq!(verify("pässwörd $ü", DJANGO_UNICODE), Verified { matches: true, must_update: true });
        assert!(!verify("Correct horse", DJANGO_1000).matches);
    }

    #[test]
    fn rejects_unusable_and_unknown() {
        let no = Verified { matches: false, must_update: false };
        assert_eq!(verify("", ""), no);
        assert_eq!(verify("x", "!NnFv3AGBsdWwGNe9S7iN5CHLtinRkhDkJyeGu8cP"), no);
        assert_eq!(verify("correct horse", "pbkdf2_sha1$1000$abcdefghijklmnopqrstuv$Lk0noscGFKIci9bRr/p5U5TdvK4="), no);
        assert_eq!(verify("x", "pbkdf2_sha256$0$salt$hash"), no);
        assert_eq!(verify("x", "pbkdf2_sha256$1000$salt"), no);
    }

    #[test]
    fn made_passwords_verify_without_update() {
        let encoded = make("s3cret");
        assert!(encoded.starts_with("pbkdf2_sha256$1000000$"));
        assert_eq!(encoded.split('$').nth(2).unwrap().len(), SALT_LEN);
        assert_eq!(verify("s3cret", &encoded), Verified { matches: true, must_update: false });
    }
}
