use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::Pipeline;
use crate::test_fixtures::WHITEBOARD_JSON;

fn specs() -> IndexMap<String, InputSpec> {
    Pipeline::from_bytes(
        WHITEBOARD_JSON.as_bytes(),
        Path::new("/p"),
        Path::new("/p/pipeline.yaml"),
    )
    .unwrap()
    .definition
    .inputs
}

#[test]
fn tokenizer_honors_quotes() {
    assert_eq!(
        tokenize(r#"mode=organic-drawon brief="two words" 'single q' plain"#),
        vec![
            "mode=organic-drawon",
            "brief=two words",
            "single q",
            "plain"
        ]
    );
    assert_eq!(tokenize("  "), Vec::<String>::new());
    assert_eq!(tokenize(r#"empty="""#), vec!["empty="]);
}

#[test]
fn free_text_binds_to_first_required_string() {
    let specs = specs();
    let raw = parse_input_args(&specs, "explain cost segregation mode=organic-drawon");
    assert_eq!(
        raw.assignments,
        vec![("mode".to_string(), "organic-drawon".to_string())]
    );
    let values = bind_inputs(&specs, raw).unwrap();
    assert_eq!(
        serde_json::to_value(&values).unwrap(),
        json!({"brief": "explain cost segregation", "mode": "organic-drawon", "includeMusic": "yes"})
    );
}

#[test]
fn undeclared_assignments_stay_in_free_text() {
    let specs = specs();
    let raw = parse_input_args(&specs, "ratio=16:9 for kids");
    assert_eq!(raw.free_text.as_deref(), Some("ratio=16:9 for kids"));
}

#[test]
fn reports_every_problem() {
    let specs = specs();
    let mut raw = parse_input_args(&specs, "mode=watercolor");
    raw.typed.insert("bogus".to_string(), json!(1));
    let err = bind_inputs(&specs, raw).unwrap_err().to_string();
    assert_eq!(
        err,
        "unknown input `bogus`; mode: must be one of stroke-reveal, organic-drawon; missing required input `brief`"
    );
}

#[test]
fn rejects_text_with_no_target() {
    let specs = specs();
    let raw = parse_input_args(&specs, "brief=topic extra words");
    let err = bind_inputs(&specs, raw).unwrap_err().to_string();
    assert!(err.starts_with("unexpected text `extra words`"), "{err}");
}

#[test]
fn parses_typed_values() {
    let mut specs: IndexMap<String, InputSpec> = serde_json::from_value(json!({
        "count": {"type": "integer"},
        "ratio": {"type": "number"},
        "flag": {"type": "boolean"},
        "tags": {"type": "array"},
        "meta": {"type": "object"}
    }))
    .unwrap();
    let raw = parse_input_args(
        &specs,
        r#"count=3 ratio=1.5 flag=yes tags=a,b meta='{"k":1}'"#,
    );
    let values = bind_inputs(&specs, raw).unwrap();
    assert_eq!(
        serde_json::to_value(&values).unwrap(),
        json!({"count": 3, "ratio": 1.5, "flag": true, "tags": ["a", "b"], "meta": {"k": 1}})
    );
    specs.shift_remove("meta");
    let err = bind_inputs(&specs, parse_input_args(&specs, "count=many"))
        .unwrap_err()
        .to_string();
    assert_eq!(err, "count: must be an integer");
}
