//! Django's form validation as upstream's forms use it: errors by field, with Django's messages.

use std::collections::BTreeMap;

use crate::py::strip;

/// Form errors by field (`__all__` for the form), with Django's messages.
pub type FieldErrors = BTreeMap<&'static str, Vec<String>>;

pub const REQUIRED: &str = "This field is required.";

pub fn error(field: &'static str, message: impl Into<String>) -> FieldErrors {
  BTreeMap::from([(field, vec![message.into()])])
}

/// A `CharField(max_length)` of a `ModelForm`: stripped, required, at most `max` characters.
pub fn char_field(errors: &mut FieldErrors, field: &'static str, value: Option<&str>, max: usize) -> String {
  let value = strip(value.unwrap_or("")).to_owned();
  let len = value.chars().count();
  if value.is_empty() {
    errors.entry(field).or_default().push(REQUIRED.into());
  } else if len > max {
    let message = format!("Ensure this value has at most {max} characters (it has {len}).");
    errors.entry(field).or_default().push(message);
  }
  value
}

/// A `TypedChoiceField` of integers.
pub fn choice_field(errors: &mut FieldErrors, field: &'static str, value: Option<i64>, choices: &[i64]) -> i64 {
  match value {
    None => {
      errors.entry(field).or_default().push(REQUIRED.into());
      0
    }
    Some(v) if !choices.contains(&v) => {
      let message = format!("Select a valid choice. {v} is not one of the available choices.");
      errors.entry(field).or_default().push(message);
      v
    }
    Some(v) => v,
  }
}
