//! Compatibility smoke tests derived from Kubernetes strategicpatch tests.
//! Source: k8s/staging/src/k8s.io/apimachinery/pkg/util/strategicpatch/patch_test.go

use serde_json::json;

use strategic_patch::{
    create_three_way_merge_map_patch, create_two_way_merge_map_patch, strategic_merge_map_patch,
    EmptySchema, Error, LookupPatchMeta, PatchMeta, PatchStrategy,
};

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

#[derive(Clone, Debug)]
struct MergeItemSchema;

impl LookupPatchMeta for MergeItemSchema {
    fn lookup_struct_meta(
        &self,
        key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        if key == "items" {
            Ok((Box::new(EmptySchema), PatchMeta::merge("name")))
        } else {
            Ok((Box::new(EmptySchema), PatchMeta::default()))
        }
    }

    fn lookup_slice_meta(
        &self,
        key: &str,
    ) -> strategic_patch::Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        self.lookup_struct_meta(key)
    }

    fn name(&self) -> &str {
        "MergeItemSchema"
    }
}

#[derive(Clone, Debug)]
struct ReplaceListSchema;

impl LookupPatchMeta for ReplaceListSchema {
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
        if key == "items" {
            Ok((
                Box::new(EmptySchema),
                PatchMeta {
                    strategies: vec![PatchStrategy::Replace],
                    merge_key: Some("name".to_string()),
                },
            ))
        } else {
            Ok((Box::new(EmptySchema), PatchMeta::default()))
        }
    }

    fn name(&self) -> &str {
        "ReplaceListSchema"
    }
}

#[derive(Clone, Debug)]
struct RetainKeysMergeListSchema;

impl LookupPatchMeta for RetainKeysMergeListSchema {
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
        if key == "items" {
            Ok((
                Box::new(EmptySchema),
                PatchMeta {
                    strategies: vec![PatchStrategy::Merge, PatchStrategy::RetainKeys],
                    merge_key: Some("name".to_string()),
                },
            ))
        } else {
            Ok((Box::new(EmptySchema), PatchMeta::default()))
        }
    }

    fn name(&self) -> &str {
        "RetainKeysMergeListSchema"
    }
}

#[test]
fn test_two_way_merge_patch_merge_list() {
    let original = json!({"items": [{"name": "a", "value": "1"}], "other": "x"})
        .as_object()
        .unwrap()
        .clone();
    let modified = json!({
        "items": [
            {"name": "a", "value": "2"},
            {"name": "b", "value": "3"}
        ],
        "other": "x"
    })
    .as_object()
    .unwrap()
    .clone();
    let patch = create_two_way_merge_map_patch(&original, &modified, &MergeItemSchema)
        .expect("patch ok");
    let expected = json!({
        "items": [
            {"name": "a", "value": "2"},
            {"name": "b", "value": "3"}
        ],
        "$setElementOrder/items": [
            {"name": "a"},
            {"name": "b"}
        ]
    })
    .as_object()
    .unwrap()
    .clone();
    assert_eq!(patch, expected);
}

#[test]
fn test_apply_two_way_patch_merge_list() {
    let original = json!({"items": [{"name": "a", "value": "1"}], "other": "x"})
        .as_object()
        .unwrap()
        .clone();
    let patch = json!({
        "items": [
            {"name": "a", "value": "2"},
            {"name": "b", "value": "3"}
        ]
    })
    .as_object()
    .unwrap()
    .clone();
    let merged =
        strategic_merge_map_patch(&original, &patch, &MergeItemSchema).expect("merge ok");
    let expected = json!({
        "items": [
            {"name": "a", "value": "2"},
            {"name": "b", "value": "3"}
        ],
        "other": "x"
    })
    .as_object()
    .unwrap()
    .clone();
    assert_eq!(merged, expected);
}

#[test]
fn test_delete_null_field() {
    let original = json!({"a": 1, "b": 2}).as_object().unwrap().clone();
    let patch = json!({"b": null}).as_object().unwrap().clone();
    let merged = strategic_merge_map_patch(&original, &patch, &TopLevelSchema).expect("merge ok");
    let expected = json!({"a": 1}).as_object().unwrap().clone();
    assert_eq!(merged, expected);
}

#[test]
fn test_three_way_conflict() {
    let original = json!({"a": 1}).as_object().unwrap().clone();
    let modified = json!({"a": 2}).as_object().unwrap().clone();
    let current = json!({"a": 3}).as_object().unwrap().clone();
    let err = create_three_way_merge_map_patch(
        &original,
        &modified,
        &current,
        &TopLevelSchema,
        false,
    )
    .expect_err("conflict expected");
    assert!(matches!(err, Error::Conflict { .. }));
}

#[test]
fn test_replace_strategy_list() {
    let original = json!({"items": [{"name": "a"}, {"name": "b"}]})
        .as_object()
        .unwrap()
        .clone();
    let modified = json!({"items": [{"name": "c"}]})
        .as_object()
        .unwrap()
        .clone();
    let patch = create_two_way_merge_map_patch(&original, &modified, &ReplaceListSchema)
        .expect("patch ok");
    let expected = json!({"items": [{"name": "c"}]})
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(patch, expected);
}

#[test]
fn test_retain_keys_merge_list_generates_retain_keys() {
    let original = json!({"items": [{"name": "a", "value": "1"}], "other": "x"})
        .as_object()
        .unwrap()
        .clone();
    let modified = json!({"items": [{"name": "a", "value": "2"}], "other": "x"})
        .as_object()
        .unwrap()
        .clone();
    let patch = create_two_way_merge_map_patch(&original, &modified, &RetainKeysMergeListSchema)
        .expect("patch ok");
    let expected = json!({
        "items": [{"$retainKeys": ["name", "value"], "name": "a", "value": "2"}],
        "$setElementOrder/items": [{"name": "a"}]
    })
    .as_object()
    .unwrap()
    .clone();
    assert_eq!(patch, expected);
}

#[test]
fn test_diff_handles_patch_directive_marker() {
    let original = json!({"$patch": "merge", "a": 1})
        .as_object()
        .unwrap()
        .clone();
    let modified = json!({"$patch": "replace", "a": 1})
        .as_object()
        .unwrap()
        .clone();
    let patch = create_two_way_merge_map_patch(&original, &modified, &TopLevelSchema)
        .expect("patch ok");
    let expected = json!({"$patch": "replace"}).as_object().unwrap().clone();
    assert_eq!(patch, expected);
}

#[test]
fn test_retain_keys_directive_map() {
    let original = json!({"a": 1, "b": 2, "c": 3}).as_object().unwrap().clone();
    let patch = json!({"$retainKeys": ["a", "c"], "a": 1})
        .as_object()
        .unwrap()
        .clone();
    let merged = strategic_merge_map_patch(&original, &patch, &TopLevelSchema).expect("merge ok");
    let expected = json!({"a": 1, "c": 3}).as_object().unwrap().clone();
    assert_eq!(merged, expected);
}

#[test]
fn test_delete_from_primitive_list_directive() {
    let original = json!({"args": ["--a", "--b", "--c"]})
        .as_object()
        .unwrap()
        .clone();
    let patch = json!({"$deleteFromPrimitiveList/args": ["--b"]})
        .as_object()
        .unwrap()
        .clone();
    let merged = strategic_merge_map_patch(&original, &patch, &TopLevelSchema).expect("merge ok");
    let expected = json!({"args": ["--a", "--c"]}).as_object().unwrap().clone();
    assert_eq!(merged, expected);
}

#[test]
fn test_set_element_order_merge_list_directive() {
    let original = json!({"items": [{"name": "b"}, {"name": "a"}]})
        .as_object()
        .unwrap()
        .clone();
    let patch = json!({"$setElementOrder/items": [{"name": "a"}, {"name": "b"}]})
        .as_object()
        .unwrap()
        .clone();
    let merged = strategic_merge_map_patch(&original, &patch, &MergeItemSchema).expect("merge ok");
    let expected = json!({"items": [{"name": "a"}, {"name": "b"}]})
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(merged, expected);
}
