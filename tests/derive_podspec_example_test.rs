#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};
use strategic_patch::{PatchSchema, create_two_way_merge_patch, strategic_merge_patch_typed};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct Container {
    name: String,
    image: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct PodSpec {
    #[patch(strategy = "merge", merge_key = "name")]
    containers: Vec<Container>,
    restart_policy: Option<String>,
}

#[test]
fn test_derive_podspec_merge_by_name() {
    let original = PodSpec {
        containers: vec![
            Container {
                name: "web".to_string(),
                image: "nginx:1.25".to_string(),
            },
            Container {
                name: "sidecar".to_string(),
                image: "busybox:1.36".to_string(),
            },
        ],
        restart_policy: Some("Always".to_string()),
    };

    let modified = PodSpec {
        containers: vec![
            Container {
                name: "web".to_string(),
                image: "nginx:1.26".to_string(),
            },
            Container {
                name: "sidecar".to_string(),
                image: "busybox:1.36".to_string(),
            },
            Container {
                name: "metrics".to_string(),
                image: "prometheus:2.52".to_string(),
            },
        ],
        restart_policy: Some("Always".to_string()),
    };

    let orig_bytes = serde_json::to_vec(&original).expect("serialize original");
    let mod_bytes = serde_json::to_vec(&modified).expect("serialize modified");
    let patch = create_two_way_merge_patch(&orig_bytes, &mod_bytes, PodSpec::schema())
        .expect("create patch");

    let merged = strategic_merge_patch_typed(&original, &patch).expect("merge ok");

    assert_eq!(merged, modified);
}
