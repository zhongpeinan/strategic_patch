#![cfg(feature = "derive")]

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use strategic_patch::{LookupPatchMeta, PatchSchema};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct WithMaps {
    labels: BTreeMap<String, String>,
    annotations: HashMap<String, String>,
}

#[test]
fn test_map_types_compile_and_are_leaf() {
    let schema = WithMaps::schema();

    assert!(schema.has_field("labels"));
    assert!(schema.has_field("annotations"));

    // Map fields should return EmptySchema (leaf types)
    let (sub_schema, meta) = schema.lookup_struct_meta("labels").unwrap();
    assert_eq!(sub_schema.name(), "Empty");
    assert!(meta.strategies.is_empty());

    let (sub_schema, meta) = schema.lookup_struct_meta("annotations").unwrap();
    assert_eq!(sub_schema.name(), "Empty");
    assert!(meta.strategies.is_empty());
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
struct WithOptionalMaps {
    node_selector: Option<BTreeMap<String, String>>,
    extra_labels: Option<HashMap<String, String>>,
}

#[test]
fn test_optional_map_types_compile() {
    let schema = WithOptionalMaps::schema();

    assert!(schema.has_field("nodeSelector"));
    assert!(schema.has_field("extraLabels"));

    let (sub_schema, _) = schema.lookup_struct_meta("nodeSelector").unwrap();
    assert_eq!(sub_schema.name(), "Empty");
}
