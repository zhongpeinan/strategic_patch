use serde_json::Value;

use crate::directives::{
    PatchDirectiveAction, apply_retain_keys, directive_keys, extract_field_from_directive,
    handle_patch_directive, is_delete_list_key, is_set_order_key,
};
use crate::error::{Error, Result};
use crate::options::MergeOptions;
use crate::schema::{JsonMap, LookupPatchMeta, PatchStrategy};
use crate::value_ext::value_kind;

pub(crate) fn merge_maps(
    original: &JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &MergeOptions,
) -> Result<JsonMap> {
    let mut patch_map = patch.clone();
    match handle_patch_directive(&patch_map)? {
        Some(PatchDirectiveAction::Replace) => {
            let mut replaced = patch_map.clone();
            replaced.remove(directive_keys::PATCH);
            return Ok(replaced);
        }
        Some(PatchDirectiveAction::Delete) => return Ok(JsonMap::new()),
        None => {}
    }

    let mut merged = original.clone();
    apply_retain_keys(&mut merged, &mut patch_map, options)?;

    merge_set_element_order(&mut merged, &mut patch_map, schema, options)?;
    preprocess_delete_from_primitive_lists(&mut merged, &mut patch_map, options)?;

    for (key, patch_value) in patch_map {
        if key == directive_keys::PATCH || key == directive_keys::RETAIN_KEYS {
            continue;
        }

        let mut patch_value = patch_value;
        if patch_value.is_null() {
            merged.remove(&key);
            if options.ignore_unmatched_nulls {
                continue;
            }
        }

        if !merged.contains_key(&key) {
            if options.ignore_unmatched_nulls {
                discard_null_values_from_patch(&mut patch_value);
            }
            if let Some(value) = remove_directives(patch_value) {
                merged.insert(key, value);
            }
            continue;
        }

        let original_value = merged.get(&key).cloned().unwrap_or(Value::Null);
        if value_kind(&original_value) != value_kind(&patch_value) {
            if options.ignore_unmatched_nulls {
                discard_null_values_from_patch(&mut patch_value);
            }
            if let Some(value) = remove_directives(patch_value) {
                merged.insert(key, value);
            }
            continue;
        }

        match (original_value, patch_value) {
            (Value::Object(original_map), Value::Object(patch_map)) => {
                let (child_schema, meta) = schema.lookup_struct_meta(&key)?;
                let (_, strategy) = meta.extract_retain_keys();
                if matches!(strategy, Some(PatchStrategy::Replace)) {
                    merged.insert(key, Value::Object(patch_map));
                } else {
                    let child =
                        merge_maps(&original_map, &patch_map, child_schema.as_ref(), options)?;
                    merged.insert(key, Value::Object(child));
                }
            }
            (Value::Array(original_list), Value::Array(patch_list)) => {
                let (child_schema, meta) = schema.lookup_slice_meta(&key)?;
                let merged_list = merge_lists(
                    &original_list,
                    &patch_list,
                    child_schema.as_ref(),
                    &meta,
                    options,
                )?;
                merged.insert(key, Value::Array(merged_list));
            }
            (_, patch_value) => {
                if let Some(value) = remove_directives(patch_value) {
                    merged.insert(key, value);
                } else {
                    merged.remove(&key);
                }
            }
        }
    }

    Ok(merged)
}

fn merge_lists(
    original: &[Value],
    patch: &[Value],
    schema: &dyn LookupPatchMeta,
    meta: &crate::schema::PatchMeta,
    options: &MergeOptions,
) -> Result<Vec<Value>> {
    if original.iter().any(|v| matches!(v, Value::Array(_)))
        || patch.iter().any(|v| matches!(v, Value::Array(_)))
    {
        return Err(Error::NoListOfLists);
    }

    let (_, strategy) = meta.extract_retain_keys();
    if matches!(strategy, Some(PatchStrategy::Replace)) {
        return Ok(patch.to_vec());
    }

    if meta.merge_key.is_none() {
        // primitive list merge (deduplicate)
        let mut merged = original.to_vec();
        merged.extend_from_slice(patch);
        merged.sort_by(|a, b| scalar_cmp(a, b));
        merged.dedup();
        return Ok(merged);
    }

    let merge_key = meta.merge_key.as_deref().unwrap();
    let (original_no_special, patch_no_special, replace) =
        merge_list_with_special_elements(original, patch, merge_key)?;
    if replace {
        return Ok(patch_no_special);
    }

    let mut merged: Vec<Value> = original_no_special;
    for patch_item in patch_no_special {
        let patch_obj = patch_item.as_object().ok_or_else(|| Error::BadArgKind {
            expected: "object".to_string(),
            actual: value_kind(&patch_item).to_string(),
        })?;
        let key_value = patch_obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
            path: merge_key.to_string(),
            merge_key: merge_key.to_string(),
        })?;
        let key_str = key_value.as_str().ok_or_else(|| Error::BadArgType {
            expected: "string".to_string(),
            actual: value_kind(key_value).to_string(),
        })?;

        let mut applied = false;
        for existing in merged.iter_mut() {
            if let Some(existing_obj) = existing.as_object() {
                let existing_key = existing_obj.get(merge_key).and_then(|v| v.as_str());
                if existing_key == Some(key_str) {
                    let merged_obj = merge_maps(existing_obj, patch_obj, schema, options)?;
                    *existing = Value::Object(merged_obj);
                    applied = true;
                    break;
                }
            }
        }
        if !applied {
            merged.push(Value::Object(patch_obj.clone()));
        }
    }

    Ok(merged)
}

fn preprocess_delete_from_primitive_lists(
    merged: &mut JsonMap,
    patch: &mut JsonMap,
    options: &MergeOptions,
) -> Result<()> {
    let keys: Vec<String> = patch.keys().cloned().collect();
    for key in keys {
        if !is_delete_list_key(&key) {
            continue;
        }
        let patch_value = patch.remove(&key).unwrap_or(Value::Null);
        if !options.merge_parallel_list {
            merged.insert(key.clone(), patch_value);
            continue;
        }
        let field =
            extract_field_from_directive(&key, directive_keys::DELETE_FROM_PRIMITIVE_LIST_PREFIX)
                .ok_or_else(|| Error::BadPatchFormatForPrimitiveList { path: key.clone() })?;
        let delete_items =
            patch_value
                .as_array()
                .ok_or_else(|| Error::BadPatchFormatForPrimitiveList {
                    path: field.to_string(),
                })?;
        let target =
            merged
                .get_mut(field)
                .ok_or_else(|| Error::BadPatchFormatForPrimitiveList {
                    path: field.to_string(),
                })?;
        let target_list =
            target
                .as_array_mut()
                .ok_or_else(|| Error::BadPatchFormatForPrimitiveList {
                    path: field.to_string(),
                })?;
        target_list.retain(|item| !delete_items.iter().any(|del| del == item));
    }
    Ok(())
}

fn merge_set_element_order(
    merged: &mut JsonMap,
    patch: &mut JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &MergeOptions,
) -> Result<()> {
    let keys: Vec<String> = patch.keys().cloned().collect();
    for key in keys {
        if !is_set_order_key(&key) {
            continue;
        }
        let set_order_value = patch.remove(&key).unwrap_or(Value::Null);
        if !options.merge_parallel_list {
            if let Some(existing) = merged.get(&key) {
                if existing != &set_order_value {
                    return Err(Error::BadPatchFormatForSetElementOrderList { path: key.clone() });
                }
            } else {
                merged.insert(key.clone(), set_order_value);
            }
            continue;
        }

        let order_list = set_order_value
            .as_array()
            .ok_or_else(|| Error::BadPatchFormatForSetElementOrderList { path: key.clone() })?;
        let field = extract_field_from_directive(&key, directive_keys::SET_ELEMENT_ORDER_PREFIX)
            .ok_or_else(|| Error::BadPatchFormatForSetElementOrderList { path: key.clone() })?;

        let original_list = merged.get(field).and_then(|v| v.as_array()).cloned();
        let patch_list = patch.get(field).and_then(|v| v.as_array()).cloned();

        let (subschema, meta) = schema.lookup_slice_meta(field)?;
        let (_, strategy) = meta.extract_retain_keys();
        if !matches!(strategy, Some(PatchStrategy::Merge)) {
            continue;
        }

        validate_patch_with_set_order_list(
            patch_list.as_deref(),
            order_list,
            meta.merge_key.as_deref(),
        )?;

        let merged_list = match (original_list, patch_list) {
            (Some(orig), None) => orig,
            (None, Some(patch_items)) => remove_directives(Value::Array(patch_items))
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default(),
            (Some(orig), Some(patch_items)) => {
                merge_lists(&orig, &patch_items, subschema.as_ref(), &meta, options)?
            }
            (None, None) => Vec::new(),
        };

        let (patch_items, server_only) = if meta.merge_key.is_none() {
            partition_primitives_by_present_in_list(&merged_list, order_list)
        } else {
            partition_maps_by_present_in_list(
                &merged_list,
                order_list,
                meta.merge_key.as_deref().unwrap(),
            )?
        };

        let normalized = normalize_element_order(
            &patch_items,
            &server_only,
            order_list,
            merged
                .get(field)
                .and_then(|v| v.as_array())
                .map_or(&[], |v| v),
            meta.merge_key.as_deref(),
        )?;
        merged.insert(field.to_string(), Value::Array(normalized));
        patch.remove(field);
    }
    Ok(())
}

fn validate_patch_with_set_order_list(
    patch_list: Option<&[Value]>,
    set_order_list: &[Value],
    merge_key: Option<&str>,
) -> Result<()> {
    let Some(patch_list) = patch_list else {
        return Ok(());
    };
    if patch_list.is_empty() || set_order_list.is_empty() {
        return Ok(());
    }
    let non_delete_list: Vec<&Value> = if merge_key.is_some() {
        patch_list
            .iter()
            .filter(|item| !is_delete_directive_item(item))
            .collect()
    } else {
        patch_list.iter().collect()
    };

    let mut patch_index = 0usize;
    let mut set_order_index = 0usize;
    while patch_index < non_delete_list.len() && set_order_index < set_order_list.len() {
        if merge_key_value_equal(
            non_delete_list[patch_index],
            &set_order_list[set_order_index],
            merge_key,
        )? {
            patch_index += 1;
        }
        set_order_index += 1;
    }
    if patch_index < non_delete_list.len() {
        return Err(Error::BadPatchFormatForSetElementOrderList {
            path: "".to_string(),
        });
    }
    Ok(())
}

fn merge_key_value_equal(left: &Value, right: &Value, merge_key: Option<&str>) -> Result<bool> {
    if merge_key.is_none() {
        return Ok(left == right);
    }
    let merge_key = merge_key.unwrap();
    let left_obj = left.as_object().ok_or_else(|| Error::BadArgKind {
        expected: "object".to_string(),
        actual: value_kind(left).to_string(),
    })?;
    let right_obj = right.as_object().ok_or_else(|| Error::BadArgKind {
        expected: "object".to_string(),
        actual: value_kind(right).to_string(),
    })?;
    let left_key = left_obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
        path: merge_key.to_string(),
        merge_key: merge_key.to_string(),
    })?;
    let right_key = right_obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
        path: merge_key.to_string(),
        merge_key: merge_key.to_string(),
    })?;
    Ok(left_key == right_key)
}

fn normalize_element_order(
    patch_items: &[Value],
    server_only: &[Value],
    patch_order: &[Value],
    server_order: &[Value],
    merge_key: Option<&str>,
) -> Result<Vec<Value>> {
    let patch_sorted = normalize_slice_order(patch_items, patch_order, merge_key)?;
    let server_sorted = normalize_slice_order(server_only, server_order, merge_key)?;
    Ok(merge_sorted_slice(
        &server_sorted,
        &patch_sorted,
        server_order,
        merge_key,
    ))
}

fn merge_sorted_slice(
    left: &[Value],
    right: &[Value],
    server_order: &[Value],
    merge_key: Option<&str>,
) -> Vec<Value> {
    let mut i = 0usize;
    let mut j = 0usize;
    let size = left.len() + right.len();
    let mut result = Vec::with_capacity(size);

    while i < left.len() || j < right.len() {
        if i >= left.len() {
            result.push(right[j].clone());
            j += 1;
            continue;
        }
        if j >= right.len() {
            result.push(left[i].clone());
            i += 1;
            continue;
        }

        let less = index_in_order(server_order, &left[i], merge_key)
            < index_in_order(server_order, &right[j], merge_key);
        if less {
            result.push(left[i].clone());
            i += 1;
        } else {
            result.push(right[j].clone());
            j += 1;
        }
    }

    result
}

fn index_in_order(order: &[Value], value: &Value, merge_key: Option<&str>) -> isize {
    for (i, item) in order.iter().enumerate() {
        if merge_key_value_equal(item, value, merge_key).unwrap_or(false) {
            return i as isize;
        }
    }
    -1
}

fn normalize_slice_order(
    to_sort: &[Value],
    order: &[Value],
    merge_key: Option<&str>,
) -> Result<Vec<Value>> {
    let mut sorted = to_sort.to_vec();
    sorted.sort_by(|a, b| {
        let ai = index_in_order(order, a, merge_key);
        let bi = index_in_order(order, b, merge_key);
        if ai >= 0 && bi >= 0 {
            ai.cmp(&bi)
        } else {
            std::cmp::Ordering::Less
        }
    });
    Ok(sorted)
}

fn partition_primitives_by_present_in_list(
    original: &[Value],
    partition_by: &[Value],
) -> (Vec<Value>, Vec<Value>) {
    let mut patch = Vec::new();
    let mut server_only = Vec::new();
    for v in original {
        if partition_by.iter().any(|p| p == v) {
            patch.push(v.clone());
        } else {
            server_only.push(v.clone());
        }
    }
    (patch, server_only)
}

fn partition_maps_by_present_in_list(
    original: &[Value],
    partition_by: &[Value],
    merge_key: &str,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut patch = Vec::new();
    let mut server_only = Vec::new();
    for v in original {
        let typed = v.as_object().ok_or_else(|| Error::BadArgKind {
            expected: "object".to_string(),
            actual: value_kind(v).to_string(),
        })?;
        let key_value = typed.get(merge_key).ok_or_else(|| Error::NoMergeKey {
            path: merge_key.to_string(),
            merge_key: merge_key.to_string(),
        })?;
        let found = partition_by.iter().any(|item| {
            item.as_object()
                .and_then(|obj| obj.get(merge_key))
                .map(|v| v == key_value)
                .unwrap_or(false)
        });
        if found {
            patch.push(v.clone());
        } else {
            server_only.push(v.clone());
        }
    }
    Ok((patch, server_only))
}

fn merge_list_with_special_elements(
    original: &[Value],
    patch: &[Value],
    merge_key: &str,
) -> Result<(Vec<Value>, Vec<Value>, bool)> {
    let mut patch_without = Vec::new();
    let mut replace = false;
    let mut original_list = original.to_vec();

    for item in patch {
        let obj = item.as_object().ok_or_else(|| Error::BadArgKind {
            expected: "object".to_string(),
            actual: value_kind(item).to_string(),
        })?;
        let Some(directive) = obj.get(directive_keys::PATCH) else {
            patch_without.push(Value::Object(obj.clone()));
            continue;
        };
        let directive = directive
            .as_str()
            .ok_or_else(|| Error::BadPatchType(format!("{directive:?}")))?;
        match directive {
            "delete" => {
                let merge_value = obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
                    path: merge_key.to_string(),
                    merge_key: merge_key.to_string(),
                })?;
                original_list.retain(|entry| {
                    entry
                        .as_object()
                        .and_then(|o| o.get(merge_key))
                        .map(|v| v != merge_value)
                        .unwrap_or(true)
                });
            }
            "replace" => {
                replace = true;
            }
            "merge" => {
                return Err(Error::BadPatchType("merge".to_string()));
            }
            other => {
                return Err(Error::BadPatchType(other.to_string()));
            }
        }
    }

    if replace {
        Ok((Vec::new(), patch_without, true))
    } else {
        Ok((original_list, patch_without, false))
    }
}

fn is_delete_directive_item(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|obj| obj.get(directive_keys::PATCH))
        .and_then(|v| v.as_str())
        .map(|v| v == "delete")
        .unwrap_or(false)
}

fn remove_directives(value: Value) -> Option<Value> {
    match value {
        Value::Object(mut map) => {
            if map.contains_key(directive_keys::PATCH) {
                return None;
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.remove(&key) {
                    if let Some(new_value) = remove_directives(child) {
                        map.insert(key, new_value);
                    }
                }
            }
            Some(Value::Object(map))
        }
        Value::Array(list) => {
            let mut result = Vec::new();
            for item in list {
                if let Some(new_item) = remove_directives(item) {
                    result.push(new_item);
                }
            }
            Some(Value::Array(result))
        }
        other => Some(other),
    }
}

fn discard_null_values_from_patch(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    if child.is_null() {
                        map.remove(&key);
                    } else {
                        discard_null_values_from_patch(child);
                    }
                }
            }
        }
        Value::Array(list) => {
            for item in list.iter_mut() {
                discard_null_values_from_patch(item);
            }
        }
        _ => {}
    }
}

fn scalar_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    scalar_string(a).cmp(&scalar_string(b))
}

fn scalar_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
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

    #[derive(Clone, Debug)]
    struct ArgsSchema;

    impl LookupPatchMeta for ArgsSchema {
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
            if key == "args" {
                Ok((
                    Box::new(EmptySchema),
                    PatchMeta {
                        strategies: vec![PatchStrategy::Merge],
                        merge_key: None,
                    },
                ))
            } else {
                Ok((Box::new(EmptySchema), PatchMeta::default()))
            }
        }

        fn name(&self) -> &str {
            "ArgsSchema"
        }
    }

    #[test]
    fn test_patch_delete_directive() {
        let original = json!({"a": 1, "b": 2}).as_object().unwrap().clone();
        let patch = json!({"$patch": "delete"}).as_object().unwrap().clone();
        let merged = merge_maps(&original, &patch, &EmptySchema, &MergeOptions::default())
            .expect("merge ok");
        assert!(merged.is_empty());
    }

    #[test]
    fn test_patch_replace_directive() {
        let original = json!({"a": 1}).as_object().unwrap().clone();
        let patch = json!({"$patch": "replace", "b": 2})
            .as_object()
            .unwrap()
            .clone();
        let merged = merge_maps(&original, &patch, &EmptySchema, &MergeOptions::default())
            .expect("merge ok");
        let expected = json!({"b": 2}).as_object().unwrap().clone();
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_retain_keys() {
        let original = json!({"a": 1, "b": 2}).as_object().unwrap().clone();
        let patch = json!({"$retainKeys": ["a"], "a": 1})
            .as_object()
            .unwrap()
            .clone();
        let merged = merge_maps(&original, &patch, &EmptySchema, &MergeOptions::default())
            .expect("merge ok");
        let expected = json!({"a": 1}).as_object().unwrap().clone();
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_delete_from_primitive_list() {
        let original = json!({"args": ["a", "b", "c"]})
            .as_object()
            .unwrap()
            .clone();
        let patch = json!({"$deleteFromPrimitiveList/args": ["b"]})
            .as_object()
            .unwrap()
            .clone();
        let merged = merge_maps(&original, &patch, &EmptySchema, &MergeOptions::default())
            .expect("merge ok");
        let expected = json!({"args": ["a", "c"]}).as_object().unwrap().clone();
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_set_element_order_primitive_list() {
        let original = json!({"args": ["a", "b", "c"]})
            .as_object()
            .unwrap()
            .clone();
        let patch = json!({"$setElementOrder/args": ["c", "a"]})
            .as_object()
            .unwrap()
            .clone();
        let merged =
            merge_maps(&original, &patch, &ArgsSchema, &MergeOptions::default()).expect("merge ok");
        let expected = json!({"args": ["b", "c", "a"]})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(merged, expected);
    }

    #[test]
    fn test_set_element_order_merge_list() {
        let original = json!({"items": [{"name": "b"}, {"name": "a"}]})
            .as_object()
            .unwrap()
            .clone();
        let patch = json!({"$setElementOrder/items": [{"name": "a"}, {"name": "b"}]})
            .as_object()
            .unwrap()
            .clone();
        let merged = merge_maps(&original, &patch, &ItemsSchema, &MergeOptions::default())
            .expect("merge ok");
        let expected = json!({"items": [{"name": "a"}, {"name": "b"}]})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(merged, expected);
    }
}
