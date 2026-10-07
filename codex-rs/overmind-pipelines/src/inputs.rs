//! Binding run inputs against a definition's `inputs` table.
//!
//! Mirrors gordon-workflows' `workflow.mjs plan`: unknown, missing and mistyped inputs are
//! rejected before anything is written, and declared defaults fill the gaps. From the slash
//! command, inputs are given as `key=value` tokens (quotes allowed); any remaining free text
//! binds to the first required string input, so `/pipeline run high-end-whiteboard explain
//! cost segregation` sets `brief`.

use indexmap::IndexMap;
use serde_json::Value;

use crate::InputSpec;
use crate::InputType;
use crate::PipelineError;

/// Raw inputs from the command line, before type checking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawInputs {
    /// `key=value` assignments, in order.
    pub assignments: Vec<(String, String)>,
    /// Remaining words joined by single spaces.
    pub free_text: Option<String>,
    /// Already-typed values (for example from a JSON inputs file).
    pub typed: IndexMap<String, Value>,
}

/// Split `args` into `key=value` assignments for declared inputs and free text.
pub fn parse_input_args(specs: &IndexMap<String, InputSpec>, args: &str) -> RawInputs {
    let mut raw = RawInputs::default();
    let mut free = Vec::new();
    for token in tokenize(args) {
        match token.split_once('=') {
            Some((key, value)) if specs.contains_key(key) => {
                raw.assignments.push((key.to_string(), value.to_string()));
            }
            _ => free.push(token),
        }
    }
    if !free.is_empty() {
        raw.free_text = Some(free.join(" "));
    }
    raw
}

/// Whitespace tokenizer that honors single and double quotes (quotes are removed).
pub fn tokenize(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for ch in args.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                in_token = true;
            }
            None if ch.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(ch);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

/// Type-check and default the inputs. Returns every problem at once.
pub fn bind_inputs(
    specs: &IndexMap<String, InputSpec>,
    raw: RawInputs,
) -> Result<IndexMap<String, Value>, PipelineError> {
    let mut errors = Vec::new();
    let mut supplied: IndexMap<String, Value> = IndexMap::new();
    for (key, value) in raw.typed {
        if !specs.contains_key(&key) {
            errors.push(format!("unknown input `{key}`"));
            continue;
        }
        supplied.insert(key, value);
    }
    for (key, text) in raw.assignments {
        let Some(spec) = specs.get(&key) else {
            errors.push(format!("unknown input `{key}`"));
            continue;
        };
        match parse_value(spec, &text) {
            Ok(value) => {
                supplied.insert(key, value);
            }
            Err(message) => errors.push(format!("{key}: {message}")),
        }
    }
    if let Some(text) = raw.free_text {
        let target = specs
            .iter()
            .find(|(key, spec)| {
                spec.required && spec.kind == InputType::String && !supplied.contains_key(*key)
            })
            .map(|(key, _)| key.clone());
        match target {
            Some(key) => {
                supplied.insert(key, Value::String(text));
            }
            None => errors.push(format!(
                "unexpected text `{text}`; give inputs as key=value ({})",
                specs.keys().cloned().collect::<Vec<_>>().join(", ")
            )),
        }
    }
    let mut values = IndexMap::new();
    for (key, spec) in specs {
        let value = supplied.shift_remove(key).or_else(|| spec.default.clone());
        let empty = match &value {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text.is_empty(),
            Some(_) => false,
        };
        if empty {
            if spec.required {
                errors.push(format!("missing required input `{key}`"));
            }
            continue;
        }
        let Some(value) = value else {
            continue;
        };
        if let Err(message) = check_type(spec, &value) {
            errors.push(format!("{key}: {message}"));
            continue;
        }
        values.insert(key.clone(), value);
    }
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(PipelineError::Inputs(errors))
    }
}

fn parse_value(spec: &InputSpec, text: &str) -> Result<Value, String> {
    match spec.kind {
        InputType::String | InputType::Other(_) => Ok(Value::String(text.to_string())),
        InputType::Enum => spec
            .values
            .iter()
            .find(|allowed| value_text(allowed) == text)
            .cloned()
            .ok_or_else(|| enum_message(spec)),
        InputType::Integer => text
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "must be an integer".to_string()),
        InputType::Number => text
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "must be a number".to_string()),
        InputType::Boolean => match text.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err("must be true or false".to_string()),
        },
        InputType::Array => match serde_json::from_str::<Value>(text) {
            Ok(value @ Value::Array(_)) => Ok(value),
            _ => Ok(Value::Array(
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| Value::String(item.to_string()))
                    .collect(),
            )),
        },
        InputType::Object => match serde_json::from_str::<Value>(text) {
            Ok(value @ Value::Object(_)) => Ok(value),
            _ => Err("must be a JSON object".to_string()),
        },
    }
}

fn check_type(spec: &InputSpec, value: &Value) -> Result<(), String> {
    let valid = match spec.kind {
        InputType::String => value.is_string(),
        InputType::Enum => spec.values.contains(value),
        InputType::Integer => value.is_i64() || value.is_u64(),
        InputType::Number => value.is_number(),
        InputType::Boolean => value.is_boolean(),
        InputType::Array => value.is_array(),
        InputType::Object => value.is_object(),
        InputType::Other(_) => true,
    };
    if valid {
        Ok(())
    } else if spec.kind == InputType::Enum {
        Err(enum_message(spec))
    } else {
        Err(format!("must be {}", spec.kind.as_str()))
    }
}

fn enum_message(spec: &InputSpec) -> String {
    let allowed: Vec<String> = spec.values.iter().map(value_text).collect();
    format!("must be one of {}", allowed.join(", "))
}

/// Display form of a value: strings without quotes, everything else as JSON.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
