//! Port of upstream's vendored `sentry/at_glitchtip_af9a700a8706/stacktraces/functions.py`: normalizing native
//! (C/C++/ObjC/Swift/Rust) function names. Used for synthetic exceptions' titles and the issue's last-frame function.

use std::sync::LazyLock;

use regex::{NoExpand, Regex};
use serde_json::{Map, Value};

use crate::grouping::py::{is_space, rstrip, strip, truthy};

static WINDECL_HASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@?(.*?)@[0-9]+$").unwrap());
static RUST_HASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"::h[a-z0-9]{16}$").unwrap());
static CPP_TRAILER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\bconst\b|&)$").unwrap());
static RUST_BLANKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Z] as )").unwrap());
static LAMBDA: LazyLock<Regex> = LazyLock::new(|| {
    // gcc | msvc | clang
    Regex::new(r"(?:\{lambda\(.*?\)\#\d+\})|(?:\blambda_[a-f0-9]{32}\b)|(?:\$_\d+\b)").unwrap()
});
static ANON_NAMESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\?A0x[a-f0-9]{8}::").unwrap());

fn is_native_platform(platform: Option<&Value>) -> bool {
    matches!(platform.and_then(Value::as_str), Some("objc" | "cocoa" | "swift" | "native" | "c"))
}

/// `get_function_name_for_frame`: the frame's function, trimmed unless a `raw_function` is present.
pub fn function_name_for_frame(frame: &Map<String, Value>, platform: Option<&Value>) -> Option<Value> {
    if frame.get("raw_function").is_some_and(truthy) {
        return frame.get("function").cloned();
    }
    let function = frame.get("function").filter(|f| truthy(f))?;
    let platform = frame.get("platform").filter(|p| truthy(p)).or(platform);
    match function {
        Value::String(s) if is_native_platform(platform) => Some(Value::String(trim_function_name(s))),
        other => Some(other.clone()),
    }
}

/// `trim_function_name` for native platforms.
pub fn trim_function_name(function: &str) -> String {
    if function == "<redacted>" || function == "<unknown>" {
        return function.to_string();
    }
    let original = function;
    let mut function = strip(function).to_string();

    if function.starts_with('[') || function.starts_with("+[") || function.starts_with("-[") {
        return function; // Objective-C
    }

    while let Some(m) = CPP_TRAILER.find(&function) {
        function = rstrip(&function[..m.start()]).to_string();
    }

    // operator<< etc. would confuse the bracket balancing below; use stand-in characters.
    function = function
        .replace("operator<<", "operator⟨⟨")
        .replace("operator<", "operator⟨")
        .replace("operator()", "operator◯")
        .replace(" -> ", " ⟿ ")
        .replace("`anonymous namespace'", "〔anonymousnamespace〕");

    function = LAMBDA.replace_all(&function, NoExpand("lambda")).into_owned();
    function = ANON_NAMESPACE.replace_all(&function, NoExpand("〔anonymousnamespace〕::")).into_owned();

    function = replace_enclosed_string(&function, '(', ')', &|value, _| {
        let value = strip(value);
        if value == "anonymous namespace" || value == "operator" { format!("({value})") } else { String::new() }
    });
    function = replace_enclosed_string(&function, '<', '>', &process_generics);

    let mut tokens = split_func_tokens(&function);
    if tokens.len() > 1 && tokens.last().map(String::as_str) == Some("<T>") {
        tokens.pop();
        if let Some(last) = tokens.last_mut() {
            last.push_str(" <T>");
        }
    }

    // Python's tokens[i - 1] with i == 0 is the last token.
    let func_token = match tokens.iter().position(|t| t == "⟿") {
        Some(0) => tokens.last(),
        Some(i) => tokens.get(i - 1),
        None => tokens.last(),
    };

    let function = match func_token {
        Some(token) if !token.is_empty() => token
            .replace('⟨', "<")
            .replace('◯', "()")
            .replace(" ⟿ ", " -> ")
            .replace("〔anonymousnamespace〕", "`anonymous namespace'"),
        _ => original.to_string(),
    };

    let function = RUST_HASH.replace_all(&function, NoExpand("")).into_owned();
    WINDECL_HASH.replace_all(&function, "${1}").into_owned()
}

fn process_generics(value: &str, start: usize) -> String {
    if value == "lambda" || LAMBDA.find(value).is_some_and(|m| m.start() == 0) {
        return format!("<{value}>");
    }
    if start > 0 {
        return "<T>".into();
    }
    let value = RUST_BLANKET.replace_all(value, NoExpand("")).into_owned();
    let value = replace_enclosed_string(&value, '<', '>', &process_generics);
    value.split(" as ").next().unwrap_or("").to_string()
}

/// Removes balanced `start`..`end` sections, replacing each outermost one by `replacement(inner, start_index)`.
fn replace_enclosed_string(s: &str, start: char, end: char, replacement: &dyn Fn(&str, usize) -> String) -> String {
    if !s.contains(start) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut depth: i64 = 0;
    let mut pair_start = 0;
    let mut rv = String::new();
    for (idx, &c) in chars.iter().enumerate() {
        if c == start {
            if depth == 0 {
                pair_start = idx;
            }
            depth += 1;
        } else if c == end {
            depth -= 1;
            if depth == 0 {
                let inner: String = chars[pair_start + 1..idx].iter().collect();
                rv.push_str(&replacement(&inner, pair_start));
            }
        } else if depth == 0 {
            rv.push(c);
        }
    }
    rv
}

/// Splits on whitespace that is not inside brackets.
fn split_func_tokens(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let slice = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    let mut buf: Vec<String> = Vec::new();
    let mut rv: Vec<Vec<String>> = Vec::new();
    let mut stack: Vec<char> = Vec::new();
    let mut end = 0;

    for (idx, &c) in chars.iter().enumerate() {
        let closing = match c {
            '(' => Some(')'),
            '{' => Some('}'),
            '[' => Some(']'),
            '<' => Some('>'),
            _ => None,
        };
        if let Some(close) = closing {
            stack.push(close);
        } else if stack.last() == Some(&c) {
            stack.pop();
            if stack.is_empty() {
                buf.push(slice(end, idx + 1));
                end = idx + 1;
            }
        } else if stack.is_empty() {
            if is_space(c) {
                if !buf.is_empty() {
                    rv.push(std::mem::take(&mut buf));
                }
            } else {
                buf.push(slice(end, idx + 1));
            }
            end = idx + 1;
        }
    }
    if !buf.is_empty() {
        rv.push(buf);
    }
    rv.into_iter().map(|parts| parts.concat()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_like_sentry() {
        // cases from Sentry's test_functions.py
        for (input, expected) in [
            ("+[FOO kek]", "+[FOO kek]"),
            ("[FOO kek]", "[FOO kek]"),
            ("std::vector<int>::push_back(int const&)", "std::vector<T>::push_back"),
            ("unsigned int main(int, char**)", "main"),
            ("void foo::bar() const", "foo::bar"),
            ("std::basic_ostream<char>& std::operator<<(std::basic_ostream<char>&, char const*)", "std::operator<<"),
            ("<actix_web::pipeline::Pipeline<S, H> as core::future::Future>::poll::h9de5d4ad3fc6c4b2",
             "actix_web::pipeline::Pipeline<T>::poll"),
            ("@objc ViewController.causeCrash(Any) -> ()", "ViewController.causeCrash"),
            ("_ZN4test4main17h2a4d3e0f2e2c0e4fE", "_ZN4test4main17h2a4d3e0f2e2c0e4fE"),
            ("@foo@12", "foo"),
            ("main::$_0::operator()() const", "main::lambda::operator()"),
        ] {
            assert_eq!(trim_function_name(input), expected, "{input}");
        }
    }
}
