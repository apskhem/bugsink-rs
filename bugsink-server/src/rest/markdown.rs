//! `events/markdown_stacktrace.py`: an event's stack trace(s) as Markdown text (frames, source, locals), for the
//! event detail's `stacktrace_md` and `/events/{id}/stacktrace/`.
//!
//! Where upstream's renderer raises on malformed event data (a frame that isn't an object, a non-string thread
//! state or exception value, ...), its response is a 500; ours renders what it can instead, as the UI API does.

use serde_json::Value;

use crate::event_display::{mark_debug_ids, stacktrace_entries, thread_stacktrace_entries};
use crate::py::{py_str, rstrip, strip, truthy};

/// `render_stacktrace_md(event, in_app_only=False, include_locals=True)`; `title` is the event's.
pub fn render(data: &Value, platform: &str, title: &str) -> String {
    let mut entries = stacktrace_entries(data);
    if entries.is_empty() {
        return "_No stacktrace available._".into();
    }
    let is_exception = entries[0]["is_exception_stacktrace"] == Value::Bool(true);
    if is_exception {
        // `apply_sourcemaps`, without uploaded sourcemaps; its errors are logged and ignored
        let _ = mark_debug_ids(&mut entries, data);
    }
    let stack_of_plates = platform != "python";
    if stack_of_plates && is_exception {
        entries.reverse();
    }
    let mut lines = Vec::new();
    if !is_exception {
        lines.extend(header_lines(title, &entries[0]));
        lines.extend(thread_lines(&entries, stack_of_plates));
        return finish(&lines);
    }
    for (i, exc) in entries.iter().enumerate() {
        if i > 0 {
            let during = "**During handling of the above exception, another exception occurred:**";
            lines.extend(["".into(), during.into(), "".into()]);
        }
        lines.extend(header_lines(title, exc));
        lines.extend(frame_lines(exc, stack_of_plates));
    }
    let threads = thread_stacktrace_entries(data);
    if threads.len() > 1 {
        lines.extend(["".into(), "# Threads".into()]);
        lines.extend(thread_lines(&threads, stack_of_plates));
    }
    finish(&lines)
}

fn finish(lines: &[String]) -> String {
    let joined: Vec<&str> = lines.iter().map(|l| rstrip(l)).collect();
    strip(&joined.join("\n")).to_owned()
}

/// `d.get(key) or ...`: a truthy value of an object's key.
fn truthy_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|v| truthy(v))
}

fn header_lines(title: &str, exc: &Value) -> Vec<String> {
    if exc["is_exception_stacktrace"] != Value::Bool(true) {
        return vec![format!("# {title}")];
    }
    let etype = truthy_get(exc, "type").map_or("Exception".into(), py_str);
    vec![format!("# {etype}"), truthy_get(exc, "value").map_or(String::new(), py_str)]
}

/// `list(x)`: a list's items, a dict's keys, a string's characters (nothing for anything else).
fn py_list(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(o)) => o.keys().map(|k| Value::String(k.clone())).collect(),
        Some(Value::String(s)) => s.chars().map(|c| Value::String(c.to_string())).collect(),
        _ => vec![],
    }
}

fn frames(exc: &Value) -> Vec<Value> {
    py_list(truthy_get(exc, "stacktrace").and_then(|s| truthy_get(s, "frames")))
}

fn frame_header(frame: &Value) -> String {
    let file = truthy_get(frame, "filename").or_else(|| truthy_get(frame, "abs_path"));
    let mut header = format!("### {}", file.map_or("<unknown>".into(), py_str));
    if let Some(lineno) = frame.get("lineno").filter(|l| !l.is_null()) {
        header.push_str(&format!(":{}", py_str(lineno)));
    }
    if let Some(function) = truthy_get(frame, "function") {
        header.push_str(&format!(" in `{}`", py_str(function)));
    }
    header.push_str(if frame.get("in_app") == Some(&Value::Bool(true)) { " [in-app]" } else { " [external]" });
    if let Some(debug_id) = truthy_get(frame, "debug_id")
        && truthy_get(frame, "mapped").is_none()
    {
        header.push_str(&format!(" (no sourcemap for debug_id {})", py_str(debug_id)));
    }
    header
}

fn context(frame: &Value, key: &str) -> Vec<String> {
    py_list(truthy_get(frame, key)).iter().map(|l| if l.is_null() { String::new() } else { py_str(l) }).collect()
}

/// Python's `int(x)` on a frame's `lineno`.
fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => match crate::py::py_int(s) {
            Some(crate::py::PyInt::Small(n)) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

fn code_gutter(frame: &Value) -> Vec<String> {
    let pre = context(frame, "pre_context");
    let ctx = frame.get("context_line").filter(|c| !c.is_null()).map(py_str);
    let post = context(frame, "post_context");
    if pre.is_empty() && ctx.is_none() && post.is_empty() {
        return vec![];
    }
    let start = match frame.get("lineno").filter(|l| !l.is_null()).and_then(py_int) {
        Some(lineno) => (lineno - pre.len() as i64).max(1),
        None => 1,
    };
    let ctx_index = ctx.as_ref().map(|_| pre.len());
    let mut all = pre;
    all.extend(ctx);
    all.extend(post);
    let last = start + all.len() as i64 - 1;
    let width = last.to_string().len().max(2);
    all.iter()
        .enumerate()
        .map(|(i, text)| {
            let marker = if Some(i) == ctx_index { "▶" } else { " " };
            format!("{marker} {:>width$} | {text}", start + i as i64)
        })
        .collect()
}

fn locals(frame: &Value) -> Vec<String> {
    let Some(Value::Object(vars)) = truthy_get(frame, "vars") else { return vec![] };
    let mut lines = vec!["".into(), "#### Locals".into(), "".into()];
    lines.extend(vars.iter().map(|(k, v)| format!("* `{k}` = `{}`", py_str(v))));
    lines
}

fn frame_lines(exc: &Value, stack_of_plates: bool) -> Vec<String> {
    let mut frames = frames(exc);
    if stack_of_plates {
        frames.reverse();
    }
    let mut lines = Vec::new();
    for frame in &frames {
        lines.push(String::new());
        lines.push(frame_header(frame));
        let code = code_gutter(frame);
        if code.is_empty() {
            lines.push("_no source context available_".into());
        } else {
            lines.extend(code);
        }
        lines.extend(locals(frame));
    }
    lines
}

fn thread_lines(threads: &[Value], stack_of_plates: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for thread in threads {
        if threads.len() > 1 {
            lines.push(String::new());
            lines.push(format!("## {}", py_str(&thread["thread_title"])));
            if let Some(description) = truthy_get(thread, "thread_description") {
                lines.push(format!("_{}_", py_str(description)));
            }
        }
        if frames(thread).is_empty() {
            lines.extend(["".into(), "_No stacktrace found._".into()]);
            continue;
        }
        lines.extend(frame_lines(thread, stack_of_plates));
    }
    lines
}
