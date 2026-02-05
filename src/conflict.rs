use serde_json::Value;

use crate::directives::{directive_keys, handle_patch_directive};
use crate::error::{Error, Result};
use crate::schema::{JsonMap, LookupPatchMeta, PatchStrategy};
use crate::value_ext::value_kind;

pub(crate) fn merging_maps_have_conflicts(
    left: &JsonMap,
    right: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<bool> {
    if patch_directive_conflicts(left, right)? {
        return Ok(true);
    }

    for (key, left_value) in left {
        if key == directive_keys::PATCH || key == directive_keys::RETAIN_KEYS {
            continue;
        }
        let Some(right_value) = right.get(key) else {
            continue;
        };
        if values_have_conflicts(left_value, right_value, key, schema)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn patch_directive_conflicts(left: &JsonMap, right: &JsonMap) -> Result<bool> {
    let left_action = handle_patch_directive(left)?;
    let right_action = handle_patch_directive(right)?;
    Ok(left_action != right_action)
}

fn values_have_conflicts(
    left: &Value,
    right: &Value,
    key: &str,
    schema: &dyn LookupPatchMeta,
) -> Result<bool> {
    if left == right {
        return Ok(false);
    }

    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            let (child_schema, _) = schema.lookup_struct_meta(key)?;
            merging_maps_have_conflicts(left_map, right_map, child_schema.as_ref())
        }
        (Value::Array(left_list), Value::Array(right_list)) => {
            let (child_schema, meta) = schema.lookup_slice_meta(key)?;
            list_conflicts(left_list, right_list, child_schema.as_ref(), &meta)
        }
        _ => Ok(true),
    }
}

fn list_conflicts(
    left: &[Value],
    right: &[Value],
    schema: &dyn LookupPatchMeta,
    meta: &crate::schema::PatchMeta,
) -> Result<bool> {
    if left.iter().any(|v| matches!(v, Value::Array(_)))
        || right.iter().any(|v| matches!(v, Value::Array(_)))
    {
        return Err(Error::NoListOfLists);
    }

    let (_, strategy) = meta.extract_retain_keys();
    if matches!(strategy, Some(PatchStrategy::Replace)) || meta.merge_key.is_none() {
        if left.len() != right.len() {
            return Ok(true);
        }
        for (l, r) in left.iter().zip(right.iter()) {
            if l != r {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    let merge_key = meta.merge_key.as_deref().unwrap();
    let left_index = index_list_by_key(left, merge_key)?;
    let right_index = index_list_by_key(right, merge_key)?;

    for (key, left_value) in left_index {
        if let Some(right_value) = right_index.get(&key) {
            match (left_value, right_value) {
                (Value::Object(left_map), Value::Object(right_map)) => {
                    if merging_maps_have_conflicts(&left_map, right_map, schema)? {
                        return Ok(true);
                    }
                }
                _ => return Ok(true),
            }
        }
    }

    Ok(false)
}

fn index_list_by_key(
    list: &[Value],
    merge_key: &str,
) -> Result<std::collections::BTreeMap<String, Value>> {
    let mut index = std::collections::BTreeMap::new();
    for value in list {
        let obj = value.as_object().ok_or_else(|| Error::BadArgKind {
            expected: "object".to_string(),
            actual: value_kind(value).to_string(),
        })?;
        let key_value = obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
            path: merge_key.to_string(),
            merge_key: merge_key.to_string(),
        })?;
        let key_str = key_value.as_str().ok_or_else(|| Error::BadArgType {
            expected: "string".to_string(),
            actual: value_kind(key_value).to_string(),
        })?;
        index.insert(key_str.to_string(), Value::Object(obj.clone()));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{EmptySchema, PatchMeta, PatchStrategy};
    use serde_json::json;

    #[derive(Clone, Debug)]
    struct ItemsSchema;

    impl LookupPatchMeta for ItemsSchema {
        fn lookup_struct_meta(
            &self,
            _key: &str,
        ) -> Result<(Box<dyn LookupPatchMeta>, crate::schema::PatchMeta)> {
            Ok((Box::new(EmptySchema), PatchMeta::default()))
        }

        fn lookup_slice_meta(
            &self,
            key: &str,
        ) -> Result<(Box<dyn LookupPatchMeta>, crate::schema::PatchMeta)> {
            if key == "items" {
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
            "ItemsSchema"
        }
    }

    #[test]
    fn test_conflict_scalar() {
        let left = json!({"a": 1}).as_object().unwrap().clone();
        let right = json!({"a": 2}).as_object().unwrap().clone();
        let conflict = merging_maps_have_conflicts(&left, &right, &EmptySchema).expect("ok");
        assert!(conflict);
    }

    #[test]
    fn test_conflict_merge_list() {
        let left = json!({"items": [{"name": "a", "x": 1}]})
            .as_object()
            .unwrap()
            .clone();
        let right = json!({"items": [{"name": "a", "x": 2}]})
            .as_object()
            .unwrap()
            .clone();
        let conflict = merging_maps_have_conflicts(&left, &right, &ItemsSchema).expect("ok");
        assert!(conflict);
    }
}
