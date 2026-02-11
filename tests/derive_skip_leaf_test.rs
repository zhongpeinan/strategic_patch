#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};
use strategic_patch::{LookupPatchMeta, PatchSchema};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct InnerMeta {
    kind: String,
    api_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct WithSkip {
    #[serde(flatten)]
    #[patch(skip)]
    meta: InnerMeta,
    name: String,
}

#[test]
fn test_skip_excludes_field() {
    let schema = WithSkip::schema();

    // Skipped field should not be in the schema
    assert!(!schema.has_field("meta"));

    // Normal field should be present
    assert!(schema.has_field("name"));
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct WithLeaf {
    #[patch(leaf)]
    timestamp: String,
    name: String,
}

#[test]
fn test_leaf_compiles_and_returns_empty_schema() {
    let schema = WithLeaf::schema();

    // Both fields should be present
    assert!(schema.has_field("timestamp"));
    assert!(schema.has_field("name"));

    // Leaf field should return EmptySchema + default PatchMeta
    let (sub_schema, meta) = schema.lookup_struct_meta("timestamp").unwrap();
    assert_eq!(sub_schema.name(), "Empty");
    assert!(meta.strategies.is_empty());
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
struct WithSkipAndRename {
    #[serde(flatten)]
    #[patch(skip)]
    type_meta: InnerMeta,
    host_name: String,
}

#[test]
fn test_skip_with_rename_all() {
    let schema = WithSkipAndRename::schema();

    // Skipped field excluded
    assert!(!schema.has_field("typeMeta"));
    assert!(!schema.has_field("type_meta"));

    // Normal field uses camelCase
    assert!(schema.has_field("hostName"));
    assert!(!schema.has_field("host_name"));
}
