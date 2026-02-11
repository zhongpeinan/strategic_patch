#![cfg(feature = "derive")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use strategic_patch::{
    LookupPatchMeta, PatchSchema, PatchStrategy, StrategicPatchResource,
    create_two_way_merge_patch, strategic_merge_patch_typed,
};

/// Mirrors a simplified real Kubernetes PodSpec with all gap features combined:
/// - rename_all (Gap 1)
/// - skip (Gap 2)
/// - BTreeMap (Gap 3)
/// - leaf (Gap 4)
/// - gvk (Gap 5)

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypeMeta {
    kind: String,
    api_version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
struct Container {
    name: String,
    image: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[serde(rename_all = "camelCase")]
#[patch(gvk = "v1/Pod")]
struct SimplePod {
    #[serde(flatten)]
    #[patch(skip)]
    type_meta: TypeMeta,
    #[patch(strategy = "merge", merge_key = "name")]
    init_containers: Vec<Container>,
    #[patch(strategy = "merge", merge_key = "name")]
    containers: Vec<Container>,
    node_selector: Option<BTreeMap<String, String>>,
    #[serde(rename = "hostPID")]
    host_pid: Option<bool>,
    #[patch(leaf)]
    creation_timestamp: Option<String>,
    restart_policy: Option<String>,
}

#[test]
fn test_full_schema_field_names() {
    let schema = SimplePod::schema();

    // Gap 1: camelCase names
    assert!(schema.has_field("initContainers"));
    assert!(schema.has_field("containers"));
    assert!(schema.has_field("nodeSelector"));
    assert!(schema.has_field("restartPolicy"));
    assert!(schema.has_field("creationTimestamp"));

    // Gap 1: explicit rename
    assert!(schema.has_field("hostPID"));
    assert!(!schema.has_field("hostPid"));

    // Gap 2: skipped field not present
    assert!(!schema.has_field("typeMeta"));
    assert!(!schema.has_field("type_meta"));

    // Rust names should not match
    assert!(!schema.has_field("init_containers"));
    assert!(!schema.has_field("node_selector"));
}

#[test]
fn test_full_schema_merge_strategies() {
    let schema = SimplePod::schema();

    let (_, meta) = schema.lookup_struct_meta("initContainers").unwrap();
    assert!(meta.has_strategy(PatchStrategy::Merge));
    assert_eq!(meta.merge_key, Some("name".to_string()));

    let (_, meta) = schema.lookup_struct_meta("containers").unwrap();
    assert!(meta.has_strategy(PatchStrategy::Merge));
    assert_eq!(meta.merge_key, Some("name".to_string()));
}

#[test]
fn test_full_schema_leaf_and_map() {
    let schema = SimplePod::schema();

    // Gap 3: BTreeMap is leaf
    let (sub, meta) = schema.lookup_struct_meta("nodeSelector").unwrap();
    assert_eq!(sub.name(), "Empty");
    assert!(meta.strategies.is_empty());

    // Gap 4: leaf attribute
    let (sub, meta) = schema.lookup_struct_meta("creationTimestamp").unwrap();
    assert_eq!(sub.name(), "Empty");
    assert!(meta.strategies.is_empty());
}

#[test]
fn test_full_gvk() {
    assert_eq!(SimplePod::gvk(), Some("v1/Pod"));
}

#[test]
fn test_full_end_to_end_merge() {
    let original = SimplePod {
        type_meta: TypeMeta {
            kind: "Pod".to_string(),
            api_version: "v1".to_string(),
        },
        init_containers: vec![Container {
            name: "init".to_string(),
            image: "busybox:1.36".to_string(),
        }],
        containers: vec![Container {
            name: "web".to_string(),
            image: "nginx:1.25".to_string(),
        }],
        node_selector: Some(BTreeMap::from([(
            "zone".to_string(),
            "us-east".to_string(),
        )])),
        host_pid: Some(false),
        creation_timestamp: Some("2024-01-01T00:00:00Z".to_string()),
        restart_policy: Some("Always".to_string()),
    };

    let modified = SimplePod {
        type_meta: TypeMeta {
            kind: "Pod".to_string(),
            api_version: "v1".to_string(),
        },
        init_containers: vec![Container {
            name: "init".to_string(),
            image: "busybox:1.37".to_string(),
        }],
        containers: vec![
            Container {
                name: "web".to_string(),
                image: "nginx:1.26".to_string(),
            },
            Container {
                name: "sidecar".to_string(),
                image: "envoy:1.30".to_string(),
            },
        ],
        node_selector: Some(BTreeMap::from([(
            "zone".to_string(),
            "us-west".to_string(),
        )])),
        host_pid: Some(true),
        creation_timestamp: Some("2024-01-01T00:00:00Z".to_string()),
        restart_policy: Some("Always".to_string()),
    };

    let orig_bytes = serde_json::to_vec(&original).expect("serialize original");
    let mod_bytes = serde_json::to_vec(&modified).expect("serialize modified");
    let patch = create_two_way_merge_patch(&orig_bytes, &mod_bytes, SimplePod::schema())
        .expect("create patch");

    let merged = strategic_merge_patch_typed(&original, &patch).expect("merge ok");

    assert_eq!(merged, modified);
}
