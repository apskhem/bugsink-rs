//! An event as upstream Bugsink 2.6.3's event pages show it: the shaping its views do before rendering
//! (`issue_event_stacktrace`, `issue_event_details`, `issue_event_breadcrumbs`, with `events/utils.py`). Rendering
//! itself (syntax highlighting, formatting values, dates) is the frontend's.
//!
//! - stack traces: exceptions (or else threads, or else the event's stacktrace), annotated where the SDK trimmed
//!   variables (`_meta`), frames of sourcemapped files marked with their debug id, the raise point marked, and newest
//!   call first for every platform but Python ("stack of plates"); other threads' stack traces
//! - details: key info, log entry, deployment and contexts (incl. browser/OS from the User-Agent), sourcemap images
//! - breadcrumbs
//!
//! Where upstream's view fails on malformed event data (a 500), this still gives what it can. Where upstream gives up
//! on a step and carries on (the `_meta` annotations, sourcemaps), it stops at the same point.
//!
//! SKIPPED(parity) (optional extras, see the README): applying uploaded sourcemaps (frames get their debug id as
//! if no sourcemap was uploaded).

use chrono::{
  DateTime,
  Utc,
};
use serde::Serialize;
use serde_json::{
  Map,
  Value,
  json,
};
use uuid::Uuid;

use crate::grouping::main_exception;
use crate::py::{
  PyInt,
  isoformat,
  py_str,
  py_uuid,
  truthy,
};
use crate::user_agent::contexts_enriched_with_user_agent;

/// What the shaping needs besides the event's payload.
pub struct EventFacts {
  pub id: Uuid,
  pub event_id: Uuid,
  pub issue_id: Uuid,
  pub calculated_type: String,
  pub calculated_value: String,
  /// The issue's (upstream shows the issue's transaction on the event page).
  pub transaction: String,
  pub timestamp: DateTime<Utc>,
  pub ingested_at: DateTime<Utc>,
  pub digested_at: DateTime<Utc>,
  pub digest_order: i32,
  pub remote_addr: Option<String>,
  pub platform: String,
  pub grouping_key: String,
}

#[derive(Debug, Serialize)]
pub struct EventDisplay {
  pub title: String,
  /// "METHOD url" of the request, if any.
  pub request_repr: String,
  pub stacktrace: Stacktraces,
  /// The key/value lists of the details tab, in upstream's order and with its labels.
  pub key_info: Vec<(String, Value)>,
  pub logentry_info: Vec<(String, Value)>,
  pub deployment_info: Vec<(String, Value)>,
  pub contexts: Value,
  pub breadcrumbs: Option<Vec<Value>>,
  /// The details tab's "Sourcemap IDs": (file name, "<debug id>  (uploaded)" or "... (not uploaded)").
  pub sourcemaps_images: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
pub struct Stacktraces {
  /// Exceptions (`is_exception_stacktrace`), else threads (with `thread_title`/`thread_description`), else the
  /// event's own stacktrace; each with `stacktrace.frames`. Frames may carry `raise_point: true`, `debug_id`, and
  /// `vars_incomplete`: how many items the SDK trimmed, per JSON pointer into `vars` ("" for `vars` itself).
  pub entries: Vec<Value>,
  /// With exceptions: all threads' stack traces (the one with frames first), if there are at least two.
  pub thread_stacktraces: Vec<Value>,
  /// Newest call first (every platform but Python); entries and frames are already in that order.
  pub stack_of_plates: bool,
  pub multiple_thread_stacktraces: bool,
}

/// The display of a stored event; None when its payload is in upstream's file storage (not readable yet).
pub async fn for_event(pool: &bugsink_db::PgPool, event_id: Uuid) -> sqlx::Result<Option<EventDisplay>> {
  let row = sqlx::query(
    "SELECT e.id, e.event_id, e.issue_id, e.project_id, e.calculated_type, e.calculated_value, i.transaction, e.timestamp,
                e.ingested_at, e.digested_at, e.digest_order, host(e.remote_addr) AS remote_addr, e.platform,
                coalesce(g.grouping_key, '') AS grouping_key,
                CASE WHEN e.storage_backend IS NULL THEN e.data END AS data
         FROM events_event e JOIN issues_issue i ON i.id = e.issue_id
         LEFT JOIN issues_grouping g ON g.id = e.grouping_id
         WHERE e.id = $1",
  )
  .bind(event_id)
  .fetch_one(pool)
  .await?;
  use sqlx::Row;
  let Some(data) = row.get::<Option<String>, _>("data") else {
    return Ok(None);
  };
  let data: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
  let facts = EventFacts {
    id: row.get("id"),
    event_id: row.get("event_id"),
    issue_id: row.get("issue_id"),
    calculated_type: row.get("calculated_type"),
    calculated_value: row.get("calculated_value"),
    transaction: row.get("transaction"),
    timestamp: row.get("timestamp"),
    ingested_at: row.get("ingested_at"),
    digested_at: row.get("digested_at"),
    digest_order: row.get("digest_order"),
    remote_addr: row.get("remote_addr"),
    platform: row.get("platform"),
    grouping_key: row.get("grouping_key"),
  };
  let mut display = display(&facts, &data);
  display.sourcemaps_images = sourcemaps_images(pool, row.get("project_id"), &data).await?;
  Ok(Some(display))
}

pub fn display(facts: &EventFacts, data: &Value) -> EventDisplay {
  EventDisplay {
    title: crate::issues::pages::title(&facts.calculated_type, &facts.calculated_value),
    request_repr: request_repr(data),
    stacktrace: stacktraces(data, facts.platform != "python"),
    key_info: key_info(facts, data),
    logentry_info: logentry_info(data),
    deployment_info: ["release", "environment", "server_name"]
      .into_iter()
      .filter_map(|k| data.get(k).map(|v| (k.to_owned(), v.clone())))
      .collect(),
    contexts: contexts_enriched_with_user_agent(data),
    breadcrumbs: values(data.get("breadcrumbs")).ok().flatten(),
    sourcemaps_images: vec![], // looked up by `for_event`
  }
}

/// `get_values`: the list itself, a dict's `values`, or a dict that is neither wrapped in a list. Err: upstream raises.
fn values(obj: Option<&Value>) -> Result<Option<Vec<Value>>, ()> {
  match obj {
    None | Some(Value::Null) => Ok(None),
    Some(Value::Array(a)) => Ok(Some(a.clone())),
    Some(Value::Object(o)) => {
      match o.get("values") {
        None => Ok(Some(vec![Value::Object(o.clone())])),
        Some(Value::Array(a)) => Ok(Some(a.clone())),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(()),
      }
    }
    Some(_) => Err(()),
  }
}

/// `_request_repr`.
fn request_repr(data: &Value) -> String {
  let Some(request) = data.get("request") else {
    return String::new();
  };
  let part = |k: &str| request.get(k).filter(|v| truthy(v)).map(py_str).unwrap_or_default();
  format!("{} {}", part("method"), part("url"))
}

fn key_info(facts: &EventFacts, data: &Value) -> Vec<(String, Value)> {
  let main = main_exception(data).ok().flatten();
  let mechanism = |key: &str| main.and_then(|e| e.get("mechanism")).and_then(|m| m.get(key)).cloned();
  let mut info = vec![
    (
      "title".to_owned(),
      json!(crate::issues::pages::title(
        &facts.calculated_type,
        &facts.calculated_value
      )),
    ),
    ("transaction".to_owned(), json!(facts.transaction)),
    ("event_id".to_owned(), json!(facts.event_id)),
    ("bugsink_internal_id".to_owned(), json!(facts.id)),
  ];
  if let Some(handled) = mechanism("handled").filter(|h| !h.is_null()) {
    info.push(("handled".to_owned(), handled));
  }
  info.extend([
    ("mechanism".to_owned(), mechanism("type").unwrap_or(Value::Null)),
    ("issue_id".to_owned(), json!(facts.issue_id)),
    ("timestamp".to_owned(), json!(isoformat(&facts.timestamp))),
    ("ingested at".to_owned(), json!(isoformat(&facts.ingested_at))),
    ("digested at".to_owned(), json!(isoformat(&facts.digested_at))),
    ("digest order".to_owned(), json!(facts.digest_order)),
    ("remote_addr".to_owned(), json!(facts.remote_addr)),
    ("grouping key".to_owned(), json!(facts.grouping_key)),
  ]);
  info
}

fn logentry_info(data: &Value) -> Vec<(String, Value)> {
  let mut info = Vec::new();
  let present = |k: &str| data.get(k).is_some_and(truthy);
  if !(present("logger") || present("logentry") || present("message")) {
    return info;
  }
  if let Some(level) = data.get("level") {
    info.push(("level".to_owned(), level.clone()));
  }
  if present("logger") {
    info.push(("logger".to_owned(), data["logger"].clone()));
  }
  let key = if data.get("logentry").is_some() {
    "logentry"
  } else {
    "message"
  };
  match data.get(key) {
    Some(Value::Object(entry)) => {
      for k in ["formatted", "message"] {
        if let Some(v) = entry.get(k).filter(|v| truthy(v)) {
          info.push((k.to_owned(), v.clone()));
        }
      }
      match entry.get("params") {
        Some(Value::Array(params)) => {
          info.extend(params.iter().enumerate().map(|(i, v)| (format!("#{i}"), v.clone())));
        }
        Some(Value::Object(params)) => info.extend(params.iter().map(|(k, v)| (k.clone(), v.clone()))),
        _ => {}
      }
    }
    Some(Value::String(s)) => info.push(("message".to_owned(), json!(s))),
    _ => {}
  }
  info
}

fn stacktraces(data: &Value, stack_of_plates: bool) -> Stacktraces {
  let mut entries = stacktrace_entries(data);
  let is_exception = entries
    .first()
    .is_some_and(|e| e["is_exception_stacktrace"] == json!(true));

  // `annotate_with_meta` and `apply_sourcemaps`: upstream catches their errors and keeps what was done until then
  if is_exception {
    let _ = annotate_with_meta(&mut entries, data);
    let _ = mark_debug_ids(&mut entries, data);
  }

  let mut thread_stacktraces = if is_exception {
    thread_stacktrace_entries(data)
  } else {
    vec![]
  };
  if thread_stacktraces.len() < 2 {
    thread_stacktraces.clear();
  }

  let to_mark = if is_exception {
    entries.last_mut()
  } else {
    entries.first_mut()
  };
  if let Some(Value::Array(frames)) = to_mark
    .and_then(|e| e.get_mut("stacktrace"))
    .and_then(|s| s.get_mut("frames"))
    && let Some(Value::Object(frame)) = frames.last_mut()
  {
    frame.insert("raise_point".into(), json!(true));
  }

  if stack_of_plates {
    if is_exception {
      entries.reverse();
    }
    for entry in &mut entries {
      if let Some(frames) = entry
        .get_mut("stacktrace")
        .filter(|s| truthy(s))
        .and_then(|s| s.get_mut("frames"))
        && truthy(frames)
        && let Some(reversed) = py_reversed(frames)
      {
        *frames = reversed;
      }
    }
    for thread in &mut thread_stacktraces {
      if let Some(Value::Object(stacktrace)) = thread.get_mut("stacktrace") {
        let frames = stacktrace
          .get("frames")
          .filter(|f| truthy(f))
          .cloned()
          .unwrap_or(json!([]));
        stacktrace.insert("frames".into(), py_reversed(&frames).unwrap_or(frames));
      }
    }
  }

  let multiple_thread_stacktraces = entries.len() > 1 && !is_exception;
  Stacktraces {
    entries,
    thread_stacktraces,
    stack_of_plates,
    multiple_thread_stacktraces,
  }
}

/// `list(reversed(v))`: a list's items, a dict's keys or a string's characters, backwards.
fn py_reversed(v: &Value) -> Option<Value> {
  Some(match v {
    Value::Array(a) => Value::Array(a.iter().rev().cloned().collect()),
    Value::Object(o) => Value::Array(o.keys().rev().map(|k| json!(k)).collect()),
    Value::String(s) => Value::Array(s.chars().rev().map(|c| json!(c.to_string())).collect()),
    _ => return None,
  })
}

/// `get_stacktrace_entries`.
pub(crate) fn stacktrace_entries(data: &Value) -> Vec<Value> {
  if let Ok(Some(exceptions)) = values(data.get("exception"))
    && !exceptions.is_empty()
  {
    return exceptions
      .into_iter()
      .map(|e| {
        let mut entry = match e {
          Value::Object(o) => o,
          _ => Map::new(),
        };
        entry.insert("is_exception_stacktrace".into(), json!(true));
        Value::Object(entry)
      })
      .collect();
  }
  let threads = thread_stacktrace_entries(data);
  if !threads.is_empty() {
    return threads;
  }
  match data.get("stacktrace") {
    Some(stacktrace) if stacktrace.get("frames").is_some_and(truthy) => {
      vec![json!({"stacktrace": stacktrace, "is_exception_stacktrace": false})]
    }
    _ => vec![],
  }
}

/// `get_thread_stacktrace_entries`: the first thread with frames goes first; none with frames, none at all.
pub(crate) fn thread_stacktrace_entries(data: &Value) -> Vec<Value> {
  let Ok(Some(threads)) = values(data.get("threads")) else {
    return vec![];
  };
  let mut entries = Vec::with_capacity(threads.len());
  let mut first_with_frames = None;
  for (i, thread) in threads.iter().enumerate() {
    let stacktrace = thread
      .get("stacktrace")
      .filter(|s| truthy(s))
      .cloned()
      .unwrap_or(json!({}));
    if first_with_frames.is_none() && stacktrace.get("frames").is_some_and(truthy) {
      first_with_frames = Some(i);
    }
    entries.push(json!({
        "stacktrace": stacktrace,
        "is_exception_stacktrace": false,
        "thread_title": thread_title(thread),
        "thread_description": thread_description(thread),
    }));
  }
  let Some(first) = first_with_frames else { return vec![] };
  let entry = entries.remove(first);
  entries.insert(0, entry);
  entries
}

fn thread_title(thread: &Value) -> String {
  let id = thread.get("id").filter(|id| !id.is_null()).map(py_str);
  let name = thread.get("name").filter(|n| truthy(n)).map(py_str);
  match (id, name) {
    (Some(id), Some(name)) => format!("Thread #{id}: {name}"),
    (None, Some(name)) => format!("Thread: {name}"),
    (Some(id), None) => format!("Thread #{id}"),
    (None, None) => "Thread".into(),
  }
}

fn thread_description(thread: &Value) -> String {
  let mut parts = Vec::new();
  if let Some(crashed) = thread.get("crashed").filter(|c| !c.is_null()) {
    parts.push(format!("errored: {}", if truthy(crashed) { "yes" } else { "no" }));
  }
  if let Some(Value::String(state)) = thread.get("state").filter(|s| truthy(s)) {
    parts.push(format!("state: {}", py_title(&state.replace('_', " "))));
  }
  parts.join(", ")
}

/// `str.title()`: each run of cased letters starts upper case and continues lower case.
fn py_title(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  let mut in_word = false;
  for c in s.chars() {
    let cased = c.is_lowercase() || c.is_uppercase();
    if cased && !in_word {
      out.extend(c.to_uppercase());
    } else if cased {
      out.extend(c.to_lowercase());
    } else {
      out.push(c);
    }
    in_word = cased;
  }
  out
}

/// Python's `int(str)` for the indices in `_meta`; None beyond what an index can be (`i64::MIN` too, as every larger
/// negative).
fn py_int(s: &str) -> Option<i64> {
  match crate::py::py_int(s)? {
    PyInt::Small(n) if n != i64::MIN => Some(n),
    _ => None,
  }
}

/// Python indexing: negative counts from the end.
fn py_index(len: usize, i: i64) -> Option<usize> {
  let i = if i < 0 { i + len as i64 } else { i };
  (0..len as i64).contains(&i).then_some(i as usize)
}

pub(crate) type Partial = Result<(), ()>;

/// `annotate_with_meta(exceptions, get_values(_meta.exception))`, in place. Err where upstream raises (and stops).
fn annotate_with_meta(entries: &mut [Value], data: &Value) -> Partial {
  let meta_values = match data.get("_meta") {
    None => return Ok(()),
    Some(Value::Object(meta)) => {
      match meta.get("exception") {
        None => return Ok(()),
        // `get_values`: only a dict with `values` gives something with `.items()`, and that must be a dict too
        Some(Value::Object(e)) => {
          match e.get("values") {
            Some(Value::Object(values)) => values,
            _ => return Err(()),
          }
        }
        Some(_) => return Err(()),
      }
    }
    Some(_) => return Err(()),
  };
  for (str_i, meta_value) in meta_values {
    let i = py_index(entries.len(), py_int(str_i).ok_or(())?).ok_or(())?;
    annotate_exception(&mut entries[i], meta_value)?;
  }
  Ok(())
}

/// `annotate_exception_with_meta` + `annotate_frame_with_meta`.
fn annotate_exception(exception: &mut Value, meta_value: &Value) -> Partial {
  // frames = exception.get("stacktrace", {}).get("frames", {})
  let exception = exception.as_object_mut().ok_or(())?;
  let mut frames = match exception.get_mut("stacktrace") {
    None => None,
    Some(Value::Object(stacktrace)) => stacktrace.get_mut("frames"),
    Some(_) => return Err(()),
  };
  // meta_frames = meta_value.get("stacktrace", {}).get("frames", {})
  let empty = Map::new();
  let meta_frames = match meta_value {
    Value::Object(m) => {
      match m.get("stacktrace") {
        None => &empty,
        Some(Value::Object(s)) => {
          match s.get("frames") {
            None => &empty,
            Some(Value::Object(f)) => f,
            Some(_) => return Err(()),
          }
        }
        Some(_) => return Err(()),
      }
    }
    _ => return Err(()),
  };
  for (str_i, meta_frame) in meta_frames {
    let i = py_int(str_i).ok_or(())?;
    let Some(Value::Array(frames)) = frames.as_deref_mut() else {
      return Err(());
    };
    let i = py_index(frames.len(), i).ok_or(())?;
    let frame = frames[i].as_object_mut().ok_or(())?;
    let vars = frame.get_mut("vars").ok_or(())?;
    let meta_vars = meta_frame.as_object().ok_or(())?.get("vars").ok_or(())?;
    // frame["vars"] = annotate_var_with_meta(...): replaced only when that returns
    let (kept, ok) = match annotate_var(vars, meta_vars, "") {
      Ok((in_original, in_result)) => (in_original.into_iter().chain(in_result).collect(), true),
      Err(in_original) => (in_original, false),
    };
    if !kept.is_empty() {
      frame.insert("vars_incomplete".into(), Value::Object(kept.into_iter().collect()));
    }
    if !ok {
      return Err(());
    }
  }
  Ok(())
}

/// (JSON pointer into `vars`, number of items trimmed there)
type Trimmed = Vec<(String, Value)>;

/// `annotate_var_with_meta`. Upstream marks a trimmed list or dict by wrapping it in a new object, which only takes
/// the original's place once the call returns; so on an error some marks survive and others don't. Ok: (marks in
/// the original object, marks only in the returned one). Err: the marks that survive in the original.
fn annotate_var(var: &mut Value, meta_var: &Value, pointer: &str) -> Result<(Trimmed, Trimmed), Trimmed> {
  let mut in_original = Vec::new();
  let mut in_result = Vec::new();
  let mut wrapped = false;
  match var {
    Value::Array(_) | Value::Object(_) => {}
    Value::String(_) => return Ok((in_original, in_result)),
    _ => return Err(in_original),
  }
  let Some(meta_var) = meta_var.as_object() else {
    return Err(in_original);
  };
  for (meta_k, meta_v) in meta_var {
    if meta_k.is_empty() {
      let have = match var {
        Value::Array(a) => a.len(),
        Value::Object(o) => o.len(),
        _ => unreachable!(),
      } as i64;
      let trimmed = match meta_v.as_object().and_then(|m| m.get("len")) {
        Some(Value::Number(n)) => {
          match n.as_i64() {
            Some(len) => json!(len - have),
            None => json!(n.as_f64().unwrap_or(f64::NAN) - have as f64),
          }
        }
        Some(Value::Bool(b)) => json!(*b as i64 - have),
        _ => return Err(in_original),
      };
      wrapped = true;
      in_result.push((pointer.to_owned(), trimmed));
      continue;
    }
    let (child, segment) = match var {
      Value::Array(a) => {
        let Some(i) = py_int(meta_k) else {
          return Err(in_original);
        };
        if !(0..a.len() as i64).contains(&i) {
          continue; // describes something that isn't there
        }
        (&mut a[i as usize], i.to_string())
      }
      Value::Object(o) => {
        match o.get_mut(meta_k) {
          Some(child) => (child, meta_k.replace('~', "~0").replace('/', "~1")),
          None => continue,
        }
      }
      _ => unreachable!(),
    };
    let child_pointer = format!("{pointer}/{segment}");
    match annotate_var(child, meta_v, &child_pointer) {
      // the child's own object is shared by the original and the wrapper; its replacement only by the latter
      Ok((child_original, child_result)) => {
        in_original.extend(child_original);
        if wrapped {
          in_result.extend(child_result)
        } else {
          in_original.extend(child_result)
        }
      }
      Err(child_original) => {
        in_original.extend(child_original);
        return Err(in_original);
      }
    }
  }
  Ok((in_original, in_result))
}

/// `debug_meta`'s sourcemap images as `{code_file: debug_id}`, as `apply_sourcemaps` and `get_sourcemap_images` find
/// them: None without images, Err where upstream's lookups fail.
fn sourcemap_debug_ids(data: &Value) -> Result<Option<Vec<(Value, Uuid)>>, ()> {
  let images = match data.get("debug_meta") {
    None => return Ok(None),
    Some(Value::Object(m)) => m.get("images").cloned().unwrap_or(json!([])),
    Some(_) => return Err(()),
  };
  if !truthy(&images) {
    return Ok(None);
  }
  let mut debug_ids: Vec<(Value, Uuid)> = Vec::new();
  // iterating: a list's items, a dict's keys, a string's characters
  let images: Vec<Value> = match images {
    Value::Array(a) => a,
    Value::Object(o) => o.keys().map(|k| json!(k)).collect(),
    Value::String(s) => s.chars().map(|c| json!(c.to_string())).collect(),
    _ => return Err(()),
  };
  for image in &images {
    // `"debug_id" in image and "code_file" in image`: a substring test on strings, membership on lists
    let has = |k: &str| {
      match image {
        Value::Object(o) => Ok(o.contains_key(k)),
        Value::String(s) => Ok(s.contains(k)),
        Value::Array(a) => Ok(a.iter().any(|v| v == k)),
        _ => Err(()),
      }
    };
    if !(has("debug_id")? && has("code_file")?) {
      continue;
    }
    let Value::Object(image) = image else { return Err(()) }; // image["type"] on a non-dict
    if image.get("type").ok_or(())? != "sourcemap" {
      continue;
    }
    let id = py_uuid(image["debug_id"].as_str().ok_or(())?).ok_or(())?;
    let code_file = image["code_file"].clone();
    if code_file.is_array() || code_file.is_object() {
      return Err(()); // unhashable
    }
    match debug_ids.iter_mut().find(|(f, _)| *f == code_file) {
      Some(existing) => existing.1 = id,
      None => debug_ids.push((code_file, id)),
    }
  }
  Ok(Some(debug_ids))
}

/// `get_sourcemap_images` before looking up the uploads: (file name, debug id) of each sourcemap image.
fn sourcemap_images(data: &Value) -> Result<Vec<(String, Uuid)>, ()> {
  sourcemap_debug_ids(data)?
    .unwrap_or_default()
    .into_iter()
    .map(|(code_file, id)| {
      // `basename()` of anything but a string fails
      let path = code_file.as_str().ok_or(())?;
      Ok((path.rsplit('/').next().unwrap_or(path).to_owned(), id))
    })
    .collect()
}

/// `get_sourcemap_images`, the event details' "Sourcemap IDs": per sourcemap image, its file name and its debug id
/// with whether a sourcemap was uploaded for it (to the project, or before uploads were per project, to none).
/// Empty where upstream's lookups fail (its page then fails).
async fn sourcemaps_images(
  pool: &bugsink_db::PgPool,
  project_id: i64,
  data: &Value,
) -> sqlx::Result<Vec<(String, String)>> {
  let Ok(images) = sourcemap_images(data) else {
    return Ok(vec![]);
  };
  if images.is_empty() {
    return Ok(vec![]);
  }
  let ids: Vec<Uuid> = images.iter().map(|(_, id)| *id).collect();
  let uploaded: Vec<Uuid> = sqlx::query_scalar(
    "SELECT debug_id FROM files_filemetadata
         WHERE debug_id = ANY($1) AND file_type = 'source_map' AND (project_id = $2 OR project_id IS NULL)",
  )
  .bind(&ids)
  .bind(project_id)
  .fetch_all(pool)
  .await?;
  Ok(
    images
      .into_iter()
      .map(|(name, id)| {
        let state = if uploaded.contains(&id) {
          " (uploaded)"
        } else {
          " (not uploaded)"
        };
        (name, format!("{} {state}", id.hyphenated()))
      })
      .collect(),
  )
}

/// `apply_sourcemaps` without uploaded sourcemaps: frames of files that an image says have a sourcemap get its debug
/// id, as a hint to upload it.
pub(crate) fn mark_debug_ids(entries: &mut [Value], data: &Value) -> Partial {
  let Some(debug_ids) = sourcemap_debug_ids(data)? else {
    return Ok(());
  };
  for entry in entries {
    let Value::Object(entry) = entry else { return Err(()) };
    let frames = match entry.get_mut("stacktrace") {
      None => continue,
      Some(Value::Object(s)) => {
        match s.get_mut("frames") {
          None => continue,
          Some(Value::Array(frames)) => frames,
          // iterating an empty dict or string yields nothing; anything else gives non-dict frames
          Some(Value::Object(o)) if o.is_empty() => continue,
          Some(Value::String(s)) if s.is_empty() => continue,
          Some(_) => return Err(()),
        }
      }
      Some(_) => return Err(()),
    };
    for frame in frames {
      let Value::Object(frame) = frame else { return Err(()) };
      let mut code_file = None;
      for key in ["abs_path", "filename"] {
        let path = frame.get(key).unwrap_or(&Value::Null);
        if path.is_array() || path.is_object() {
          return Err(());
        }
        if let Some((f, id)) = debug_ids.iter().find(|(f, _)| f == path) {
          code_file = Some((f.clone(), *id));
          break;
        }
      }
      if let Some((f, id)) = code_file
        && !f.is_null()
      {
        frame.insert("debug_id".into(), json!(id.hyphenated().to_string()));
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn python_ints_titles_and_uuids() {
    assert_eq!(py_int(" 1_0 "), Some(10));
    assert_eq!(py_int("-1"), Some(-1));
    assert_eq!(py_int("1__0"), None);
    assert_eq!(py_int("x"), None);
    assert_eq!(py_title("in progress_x"), "In Progress_X");
    assert_eq!(py_title("they're WAITING"), "They'Re Waiting");
    assert_eq!(
      py_uuid("{urn:uuid:12345678-1234-5678-1234-567812345678}")
        .unwrap()
        .simple()
        .to_string(),
      "12345678123456781234567812345678"
    );
    assert_eq!(py_uuid("123"), None);
  }

  #[test]
  fn marks_trimmed_vars() {
    let data = json!({
        "exception": {"values": [{"type": "E", "stacktrace": {"frames": [
            {"function": "f", "vars": {"a": [1, 2], "b": {"x": 1}, "c": "[Filtered]"}}]}}]},
        "_meta": {"exception": {"values": {"0": {"stacktrace": {"frames": {"0": {"vars": {
            "": {"len": 5}, "a": {"": {"len": 10}}, "b": {"y": {}}, "c": {"": {"rem": [["!config", "s"]]}}}}}}}}}},
    });
    let s = stacktraces(&data, false);
    let frame = &s.entries[0]["stacktrace"]["frames"][0];
    assert_eq!(frame["vars_incomplete"], json!({"": 2, "/a": 8}));
    assert_eq!(frame["raise_point"], json!(true));
  }

  #[test]
  fn keeps_what_upstream_keeps_when_annotating_fails() {
    // "b.x" is a number: upstream raises. Marks made before `vars` itself was wrapped survive, later ones don't.
    let meta = |vars: Value| {
      json!({"exception": [{"stacktrace": {"frames": [{"vars": {"a": [1, 2], "b": {"x": 1}, "c": [3]}}]}}],
                   "_meta": {"exception": {"values": {"0": {"stacktrace": {"frames": {"0": {"vars": vars}}}}}}}})
    };
    let incomplete =
      |data: Value| stacktraces(&data, false).entries[0]["stacktrace"]["frames"][0]["vars_incomplete"].clone();
    let a = json!({"": {"len": 10}});
    assert_eq!(
      incomplete(meta(json!({"a": a, "": {"len": 5}, "b": {"x": {}}}))),
      json!({"/a": 8})
    );
    assert_eq!(
      incomplete(meta(json!({"": {"len": 5}, "a": a, "b": {"x": {}}}))),
      Value::Null
    );
    assert_eq!(
      incomplete(meta(json!({"c": {"": {"len": 4}}, "a": a, "b": {"x": {}}}))),
      json!({"/c": 3, "/a": 8})
    );
  }

  #[test]
  fn stack_of_plates_reverses_exceptions_and_frames() {
    let data = json!({"exception": [
            {"type": "A", "stacktrace": {"frames": [{"function": "a1"}, {"function": "a2"}]}},
            {"type": "B", "stacktrace": {"frames": [{"function": "b1"}, {"function": "b2"}]}}]});
    let s = stacktraces(&data, true);
    assert_eq!(s.entries[0]["type"], "B");
    assert_eq!(
      s.entries[0]["stacktrace"]["frames"][0],
      json!({"function": "b2", "raise_point": true})
    );
    let s = stacktraces(&data, false);
    assert_eq!(
      s.entries[1]["stacktrace"]["frames"][1],
      json!({"function": "b2", "raise_point": true})
    );
  }
}
