# strategic_patch

Rust implementation of Kubernetes Strategic Merge Patch compatible behavior.

## Map API

Note: top-level operations require a non-Empty schema. `EmptySchema` is only intended
for nested fields; top-level use returns `Error::UnsupportedStrategicMergePatchFormat`.

```rust
use serde_json::json;
use strategic_patch::{create_two_way_merge_map_patch, strategic_merge_map_patch, EmptySchema, LookupPatchMeta, PatchMeta};

#[derive(Clone, Debug)]
struct TopLevelSchema;

impl LookupPatchMeta for TopLevelSchema {
    fn lookup_struct_meta(
        &self,
        _key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn lookup_slice_meta(
        &self,
        _key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn name(&self) -> &str {
        "TopLevelSchema"
    }
}

let original = json!({"a": 1}).as_object().unwrap().clone();
let modified = json!({"a": 2}).as_object().unwrap().clone();
let patch = create_two_way_merge_map_patch(&original, &modified, &TopLevelSchema)?;
let merged = strategic_merge_map_patch(&original, &patch, &TopLevelSchema)?;
assert_eq!(merged, modified);
```

## Bytes API

```rust
use strategic_patch::{create_two_way_merge_patch, strategic_merge_patch, EmptySchema, LookupPatchMeta, PatchMeta};

#[derive(Clone, Debug)]
struct TopLevelSchema;

impl LookupPatchMeta for TopLevelSchema {
    fn lookup_struct_meta(
        &self,
        _key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn lookup_slice_meta(
        &self,
        _key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn name(&self) -> &str {
        "TopLevelSchema"
    }
}

let original = br#"{"a":1}"#;
let modified = br#"{"a":2}"#;
let patch = create_two_way_merge_patch(original, modified, &TopLevelSchema)?;
let merged = strategic_merge_patch(original, &patch, &TopLevelSchema)?;
```

## Typed API

Typed merge will apply `T::preconditions()` to the incoming patch before merging.

```rust
use serde::{Deserialize, Serialize};
use strategic_patch::{create_two_way_merge_patch, strategic_merge_patch_typed, PatchSchema};

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

let orig_bytes = serde_json::to_vec(&original)?;
let mod_bytes = serde_json::to_vec(&modified)?;
let patch = create_two_way_merge_patch(&orig_bytes, &mod_bytes, PodSpec::schema())?;
let merged = strategic_merge_patch_typed(&original, patch)?;
assert_eq!(merged, modified);
```

## Realistic Example (PodSpec-like, derive)

This example uses a list merge by `name` (like `containers` in Kubernetes).

```rust
use serde::{Deserialize, Serialize};
use strategic_patch::{
    create_three_way_merge_patch, create_two_way_merge_patch, strategic_merge_patch_typed,
    PatchSchema,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct Container {
    name: String,
    image: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct PodSpec {
    #[patch(strategy = "merge", merge_key = "name")]
    containers: Vec<Container>,
}

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
    ],
};

let current = PodSpec {
    containers: vec![
        Container {
            name: "web".to_string(),
            image: "nginx:1.27".to_string(), // changed elsewhere
        },
        Container {
            name: "sidecar".to_string(),
            image: "busybox:1.36".to_string(),
        },
    ],
};

let orig_bytes = serde_json::to_vec(&original)?;
let mod_bytes = serde_json::to_vec(&modified)?;
let curr_bytes = serde_json::to_vec(&current)?;

let patch = create_two_way_merge_patch(&orig_bytes, &mod_bytes, PodSpec::schema())?;
let merged = strategic_merge_patch_typed(&original, patch)?;
assert_eq!(merged, modified);

let err = create_three_way_merge_patch(
    &orig_bytes,
    &mod_bytes,
    &curr_bytes,
    PodSpec::schema(),
    false,
).expect_err("conflict expected");
assert!(matches!(err, strategic_patch::Error::Conflict { .. }));
```
