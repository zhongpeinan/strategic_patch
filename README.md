# strategic_patch

Rust implementation of Kubernetes Strategic Merge Patch compatible behavior.

## Map API

```rust
use serde_json::json;
use strategic_patch::{create_two_way_merge_map_patch, strategic_merge_map_patch, EmptySchema};

let original = json!({"a": 1}).as_object().unwrap().clone();
let modified = json!({"a": 2}).as_object().unwrap().clone();
let patch = create_two_way_merge_map_patch(&original, &modified, &EmptySchema)?;
let merged = strategic_merge_map_patch(&original, &patch, &EmptySchema)?;
assert_eq!(merged, modified);
```

## Bytes API

```rust
use strategic_patch::{create_two_way_merge_patch, strategic_merge_patch, EmptySchema};

let original = br#"{"a":1}"#;
let modified = br#"{"a":2}"#;
let patch = create_two_way_merge_patch(original, modified, &EmptySchema)?;
let merged = strategic_merge_patch(original, &patch, &EmptySchema)?;
```

## Typed API

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
