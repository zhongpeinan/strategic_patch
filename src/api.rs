use serde::{de::DeserializeOwned, Serialize};

use crate::conflict::merging_maps_have_conflicts as conflicts_inner;
use crate::diff::diff_maps;
use crate::error::{Error, Result};
use crate::merge::merge_maps;
use crate::options::{DiffOptions, MergeOptions};
use crate::schema::{JsonMap, LookupPatchMeta, PreconditionFn, StrategicPatchResource};
use crate::sort::sort_merge_lists_by_name_map;

pub fn create_two_way_merge_map_patch(
    original: &JsonMap,
    modified: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<JsonMap> {
    create_two_way_merge_map_patch_with_preconditions(original, modified, schema, &[])
}

pub fn create_two_way_merge_map_patch_with_preconditions(
    original: &JsonMap,
    modified: &JsonMap,
    schema: &dyn LookupPatchMeta,
    preconditions: &[PreconditionFn],
) -> Result<JsonMap> {
    let mut options = DiffOptions::default();
    options.set_element_order = true;
    let patch = diff_maps(original, modified, schema, &options)?;
    for precondition in preconditions {
        if !precondition(&patch) {
            return Err(Error::PreconditionFailed("precondition failed".to_string()));
        }
    }
    Ok(patch)
}

pub fn strategic_merge_map_patch(
    original: &JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<JsonMap> {
    strategic_merge_map_patch_with_options(original, patch, schema, &MergeOptions::default())
}

pub fn strategic_merge_map_patch_with_options(
    original: &JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &MergeOptions,
) -> Result<JsonMap> {
    merge_maps(original, patch, schema, options)
}

pub fn create_three_way_merge_map_patch(
    original: &JsonMap,
    modified: &JsonMap,
    current: &JsonMap,
    schema: &dyn LookupPatchMeta,
    overwrite: bool,
) -> Result<JsonMap> {
    let mut delta_opts = DiffOptions::default();
    delta_opts.ignore_deletions = true;
    let delta = diff_maps(current, modified, schema, &delta_opts)?;

    let mut deletions_opts = DiffOptions::default();
    deletions_opts.ignore_changes_and_additions = true;
    let deletions = diff_maps(original, modified, schema, &deletions_opts)?;

    let merge_opts = MergeOptions {
        merge_parallel_list: false,
        ignore_unmatched_nulls: false,
    };
    let patch = merge_maps(&deletions, &delta, schema, &merge_opts)?;

    if !overwrite {
        let changed = diff_maps(original, current, schema, &DiffOptions::default())?;
        if conflicts_inner(&patch, &changed, schema)? {
            let patch_str = serde_json::to_string(&patch).unwrap_or_else(|_| "<patch>".into());
            let current_str = serde_json::to_string(&changed).unwrap_or_else(|_| "<current>".into());
            return Err(Error::Conflict {
                patch: patch_str,
                current: current_str,
            });
        }
    }

    Ok(patch)
}

pub fn merge_strategic_merge_map_patch(
    schema: &dyn LookupPatchMeta,
    patches: &[&JsonMap],
) -> Result<JsonMap> {
    let mut merged = JsonMap::new();
    let merge_opts = MergeOptions {
        merge_parallel_list: false,
        ignore_unmatched_nulls: false,
    };
    for patch in patches {
        merged = merge_maps(&merged, patch, schema, &merge_opts)?;
    }
    Ok(merged)
}

pub fn merging_maps_have_conflicts(
    left: &JsonMap,
    right: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<bool> {
    conflicts_inner(left, right, schema)
}

pub fn create_two_way_merge_patch(
    original: &[u8],
    modified: &[u8],
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let modi: JsonMap = serde_json::from_slice(modified)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let patch = create_two_way_merge_map_patch(&orig, &modi, schema)?;
    Ok(serde_json::to_vec(&patch)?)
}

pub fn strategic_merge_patch(
    original: &[u8],
    patch: &[u8],
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let p: JsonMap =
        serde_json::from_slice(patch).map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let result = strategic_merge_map_patch(&orig, &p, schema)?;
    Ok(serde_json::to_vec(&result)?)
}

pub fn create_three_way_merge_patch(
    original: &[u8],
    modified: &[u8],
    current: &[u8],
    schema: &dyn LookupPatchMeta,
    overwrite: bool,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let modi: JsonMap = serde_json::from_slice(modified)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let curr: JsonMap = serde_json::from_slice(current)
        .map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    let patch = create_three_way_merge_map_patch(&orig, &modi, &curr, schema, overwrite)?;
    Ok(serde_json::to_vec(&patch)?)
}

pub fn sort_merge_lists_by_name(value: &[u8], schema: &dyn LookupPatchMeta) -> Result<Vec<u8>> {
    let mut map: JsonMap =
        serde_json::from_slice(value).map_err(|e| Error::BadJsonDoc(e.to_string()))?;
    sort_merge_lists_by_name_map(&mut map, schema)?;
    Ok(serde_json::to_vec(&map)?)
}

pub fn strategic_merge_patch_typed<T>(original: &T, patch: &[u8]) -> Result<T>
where
    T: StrategicPatchResource + Serialize + DeserializeOwned,
{
    let orig_bytes = serde_json::to_vec(original)?;
    let result_bytes = strategic_merge_patch(&orig_bytes, patch, T::schema())?;
    Ok(serde_json::from_slice(&result_bytes)?)
}

pub fn create_three_way_merge_patch_typed<T>(
    original: &T,
    modified: &T,
    current: &T,
    overwrite: bool,
) -> Result<Vec<u8>>
where
    T: StrategicPatchResource + Serialize,
{
    let orig = serde_json::to_vec(original)?;
    let modi = serde_json::to_vec(modified)?;
    let curr = serde_json::to_vec(current)?;
    create_three_way_merge_patch(&orig, &modi, &curr, T::schema(), overwrite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EmptySchema;
    use serde_json::json;

    #[test]
    fn test_three_way_merge_no_conflict() {
        let original = json!({"a": 1}).as_object().unwrap().clone();
        let modified = json!({"a": 2}).as_object().unwrap().clone();
        let current = json!({"a": 1}).as_object().unwrap().clone();
        let patch = create_three_way_merge_map_patch(
            &original,
            &modified,
            &current,
            &EmptySchema,
            false,
        )
        .expect("ok");
        let expected = json!({"a": 2}).as_object().unwrap().clone();
        assert_eq!(patch, expected);
    }

    #[test]
    fn test_merge_multiple_patches() {
        let p1 = json!({"a": 1}).as_object().unwrap().clone();
        let p2 = json!({"b": 2}).as_object().unwrap().clone();
        let merged = merge_strategic_merge_map_patch(&EmptySchema, &[&p1, &p2]).expect("ok");
        let expected = json!({"a": 1, "b": 2}).as_object().unwrap().clone();
        assert_eq!(merged, expected);
    }
}
