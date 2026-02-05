use serde_json::Value;

use crate::error::{Error, Result};
use crate::schema::{JsonMap, LookupPatchMeta, PatchStrategy};
use crate::value_ext::value_kind;

pub(crate) fn sort_merge_lists_by_name_map(
    map: &mut JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<()> {
    for (key, value) in map.iter_mut() {
        match value {
            Value::Object(obj) => {
                let (child_schema, _) = schema.lookup_struct_meta(key)?;
                sort_merge_lists_by_name_map(obj, child_schema.as_ref())?;
            }
            Value::Array(list) => {
                if list.iter().any(|v| matches!(v, Value::Array(_))) {
                    return Err(Error::NoListOfLists);
                }
                let (child_schema, meta) = schema.lookup_slice_meta(key)?;
                let (_, strategy) = meta.extract_retain_keys();
                if matches!(strategy, Some(PatchStrategy::Replace)) || meta.merge_key.is_none() {
                    continue;
                }
                let merge_key = meta.merge_key.as_deref().unwrap();
                for item in list.iter_mut() {
                    if let Value::Object(obj) = item {
                        sort_merge_lists_by_name_map(obj, child_schema.as_ref())?;
                    }
                    validate_merge_key_list(item, merge_key)?;
                }
                list.sort_by(|a, b| {
                    let a_key = extract_merge_key(a, merge_key);
                    let b_key = extract_merge_key(b, merge_key);
                    a_key.cmp(&b_key)
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn extract_merge_key(value: &Value, merge_key: &str) -> String {
    value
        .as_object()
        .and_then(|obj| obj.get(merge_key))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn validate_merge_key_list(value: &Value, merge_key: &str) -> Result<()> {
    let obj = value.as_object().ok_or_else(|| Error::BadArgKind {
        expected: "object".to_string(),
        actual: value_kind(value).to_string(),
    })?;
    let key_value = obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
        path: merge_key.to_string(),
        merge_key: merge_key.to_string(),
    })?;
    if !key_value.is_string() {
        return Err(Error::BadArgType {
            expected: "string".to_string(),
            actual: value_kind(key_value).to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::EmptySchema;
    use serde_json::json;

    #[test]
    fn test_sort_merge_lists_by_name_map_noop_on_empty_schema() {
        let mut value = json!({"items": [{"name": "b"}, {"name": "a"}]})
            .as_object()
            .unwrap()
            .clone();
        sort_merge_lists_by_name_map(&mut value, &EmptySchema).expect("sort ok");
        let expected = json!({"items": [{"name": "b"}, {"name": "a"}]})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(value, expected);
    }
}
