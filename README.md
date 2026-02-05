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
use strategic_patch::{strategic_merge_patch_typed, StrategicPatchResource};

#[derive(Clone, Debug, Serialize, Deserialize, StrategicPatchResource)]
struct MyType {
    a: i32,
}

let original = MyType { a: 1 };
let patch = br#"{"a":2}"#;
let merged = strategic_merge_patch_typed(&original, patch)?;
assert_eq!(merged.a, 2);
```

## Realistic Example (PodSpec-like)

This example uses a list merge by `name` (like `containers` in Kubernetes).

```rust
use serde::{Deserialize, Serialize};
use serde_json::json;
use strategic_patch::{
    create_three_way_merge_map_patch, create_two_way_merge_map_patch,
    strategic_merge_map_patch, EmptySchema, LookupPatchMeta, PatchMeta, PatchStrategy,
};

#[derive(Clone, Debug)]
struct PodSpecSchema;

impl LookupPatchMeta for PodSpecSchema {
    fn lookup_struct_meta(
        &self,
        _key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn lookup_slice_meta(
        &self,
        key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        if key == "containers" {
            Ok((
                Box::new(EmptySchema),
                PatchMeta {
                    strategies: vec![PatchStrategy::Merge],
                    merge_key: Some("name".to_string()),
                },
            ))
        } else {
            Ok((Box::new(EmptySchema), PatchMeta::default()))
        }
    }

    fn name(&self) -> &str {
        "PodSpec"
    }
}

// Map API: two-way diff + apply
let original = json!({
    "containers": [
        {"name": "web", "image": "nginx:1.25"},
        {"name": "sidecar", "image": "busybox:1.36"}
    ]
}).as_object().unwrap().clone();

let modified = json!({
    "containers": [
        {"name": "web", "image": "nginx:1.26"},
        {"name": "sidecar", "image": "busybox:1.36"}
    ]
}).as_object().unwrap().clone();

let patch = create_two_way_merge_map_patch(&original, &modified, &PodSpecSchema)?;
let merged = strategic_merge_map_patch(&original, &patch, &PodSpecSchema)?;
assert_eq!(merged, modified);

// Three-way: detect conflict when current diverges
let current = json!({
    "containers": [
        {"name": "web", "image": "nginx:1.27"}, // changed elsewhere
        {"name": "sidecar", "image": "busybox:1.36"}
    ]
}).as_object().unwrap().clone();

let err = create_three_way_merge_map_patch(
    &original,
    &modified,
    &current,
    &PodSpecSchema,
    false,
).expect_err("conflict expected");
assert!(matches!(err, strategic_patch::Error::Conflict { .. }));
```
