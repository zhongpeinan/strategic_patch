#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};
use strategic_patch::{LookupPatchMeta, PatchSchema, PatchStrategy};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct Item {
    name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
struct TestRenameAll {
    #[patch(strategy = "merge", merge_key = "name")]
    init_containers: Vec<Item>,
    simple_field: String,
    restart_policy: Option<String>,
}

#[test]
fn test_camel_case_rename_all() {
    let schema = TestRenameAll::schema();

    // JSON camelCase names should match
    assert!(schema.has_field("initContainers"));
    assert!(schema.has_field("simpleField"));
    assert!(schema.has_field("restartPolicy"));

    // Rust snake_case names should NOT match
    assert!(!schema.has_field("init_containers"));
    assert!(!schema.has_field("simple_field"));
    assert!(!schema.has_field("restart_policy"));
}

#[test]
fn test_camel_case_lookup_returns_correct_meta() {
    let schema = TestRenameAll::schema();

    let (_, meta) = schema.lookup_struct_meta("initContainers").unwrap();
    assert!(meta.has_strategy(PatchStrategy::Merge));
    assert_eq!(meta.merge_key, Some("name".to_string()));

    let (_, meta) = schema.lookup_struct_meta("simpleField").unwrap();
    assert!(meta.strategies.is_empty());
    assert_eq!(meta.merge_key, None);
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
struct TestExplicitRename {
    #[serde(rename = "hostPID")]
    host_pid: bool,
    normal_field: String,
}

#[test]
fn test_explicit_rename_overrides_rename_all() {
    let schema = TestExplicitRename::schema();

    // Explicit rename takes precedence
    assert!(schema.has_field("hostPID"));
    assert!(!schema.has_field("hostPid")); // camelCase would produce this
    assert!(!schema.has_field("host_pid"));

    // Normal field still uses rename_all
    assert!(schema.has_field("normalField"));
    assert!(!schema.has_field("normal_field"));
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct TestNoRename {
    my_field: String,
    another: i32,
}

#[test]
fn test_no_rename_uses_rust_names() {
    let schema = TestNoRename::schema();

    // Without rename_all, Rust field names should be used
    assert!(schema.has_field("my_field"));
    assert!(schema.has_field("another"));
    assert!(!schema.has_field("myField"));
}
