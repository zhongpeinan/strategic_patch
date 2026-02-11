use serde_json::Value;

use crate::directives::directive_keys;
use crate::error::{Error, Result};
use crate::options::DiffOptions;
use crate::schema::{JsonMap, LookupPatchMeta, PatchStrategy};
use crate::value_ext::value_kind;

pub(crate) fn diff_maps(
    original: &JsonMap,
    modified: &JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &DiffOptions,
) -> Result<JsonMap> {
    let mut patch = JsonMap::new();
    let mut retain_keys: Vec<String> = Vec::new();

    for (key, modified_value) in modified {
        if key == directive_keys::PATCH {
            if let Some(original_value) = original.get(key) {
                if original_value != modified_value {
                    patch.insert(key.clone(), modified_value.clone());
                }
            }
            continue;
        }
        if options.build_retain_keys_directive && !modified_value.is_null() {
            retain_keys.push(key.clone());
        }

        match original.get(key) {
            None => {
                if !options.ignore_changes_and_additions {
                    patch.insert(key.clone(), modified_value.clone());
                }
            }
            Some(original_value) => {
                let modified_kind = value_kind(modified_value);
                let original_kind = value_kind(original_value);
                if modified_kind != original_kind {
                    if !options.ignore_changes_and_additions {
                        patch.insert(key.clone(), modified_value.clone());
                    }
                    continue;
                }

                match (original_value, modified_value) {
                    (Value::Object(orig_map), Value::Object(mod_map)) => {
                        let (child_schema, meta) = schema.lookup_struct_meta(key)?;
                        let (retain_keys_directive, strategy) = meta.extract_retain_keys();
                        if matches!(strategy, Some(PatchStrategy::Replace)) {
                            if !options.ignore_changes_and_additions
                                && original_value != modified_value
                            {
                                patch.insert(key.clone(), modified_value.clone());
                            }
                            continue;
                        }
                        let mut child_options = options.clone();
                        if retain_keys_directive {
                            child_options.build_retain_keys_directive = true;
                        }
                        let child_patch =
                            diff_maps(orig_map, mod_map, child_schema.as_ref(), &child_options)?;
                        if !child_patch.is_empty() {
                            patch.insert(key.clone(), Value::Object(child_patch));
                        }
                    }
                    (Value::Array(orig_list), Value::Array(mod_list)) => {
                        let (child_schema, meta) = schema.lookup_slice_meta(key)?;
                        let (retain_keys_directive, _) = meta.extract_retain_keys();
                        let mut child_options = options.clone();
                        if retain_keys_directive {
                            child_options.build_retain_keys_directive = true;
                        }
                        let list_result = diff_lists(
                            orig_list,
                            mod_list,
                            child_schema.as_ref(),
                            &meta,
                            &child_options,
                        )?;
                        if let Some(patch_value) = list_result.patch {
                            patch.insert(key.clone(), patch_value);
                        }
                        if let Some(delete_list) = list_result.delete_list {
                            let directive_key = format!(
                                "{}/{}",
                                directive_keys::DELETE_FROM_PRIMITIVE_LIST_PREFIX,
                                key
                            );
                            patch.insert(directive_key, delete_list);
                        }
                        if let Some(set_order) = list_result.set_order {
                            let directive_key =
                                format!("{}/{}", directive_keys::SET_ELEMENT_ORDER_PREFIX, key);
                            patch.insert(directive_key, set_order);
                        }
                    }
                    _ => {
                        if !options.ignore_changes_and_additions && original_value != modified_value
                        {
                            patch.insert(key.clone(), modified_value.clone());
                        }
                    }
                }
            }
        }
    }

    if !options.ignore_deletions {
        for key in original.keys() {
            if !modified.contains_key(key) {
                patch.insert(key.clone(), Value::Null);
            }
        }
    }

    if options.build_retain_keys_directive {
        let retain_needed = !patch.is_empty()
            || original
                .keys()
                .any(|key| !retain_keys.iter().any(|k| k == key));
        if retain_needed {
            retain_keys.sort_by(|a, b| a.cmp(b));
            let keys_value = retain_keys
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>();
            patch.insert(
                directive_keys::RETAIN_KEYS.to_string(),
                Value::Array(keys_value),
            );
        }
    }

    Ok(patch)
}

fn diff_lists(
    original: &[Value],
    modified: &[Value],
    schema: &dyn LookupPatchMeta,
    meta: &crate::schema::PatchMeta,
    options: &DiffOptions,
) -> Result<DiffListResult> {
    if original.iter().any(|v| matches!(v, Value::Array(_)))
        || modified.iter().any(|v| matches!(v, Value::Array(_)))
    {
        return Err(Error::NoListOfLists);
    }

    let (_, strategy) = meta.extract_retain_keys();
    if !matches!(strategy, Some(PatchStrategy::Merge)) {
        if !options.ignore_changes_and_additions && original != modified {
            return Ok(DiffListResult::new(
                Some(Value::Array(modified.to_vec())),
                None,
                None,
            ));
        }
        return Ok(DiffListResult::new(None, None, None));
    }

    let merge_key = meta.merge_key.as_deref();
    if merge_key.is_none() {
        let (add_list, delete_list) = diff_lists_of_scalars(original, modified, options)?;
        let set_order =
            build_set_element_order_list_scalars(original, modified, &delete_list, options);
        let patch_value = if add_list.is_empty() {
            None
        } else {
            Some(Value::Array(add_list))
        };
        let delete_value = if delete_list.is_empty() {
            None
        } else {
            Some(Value::Array(delete_list))
        };
        return Ok(DiffListResult::new(patch_value, delete_value, set_order));
    }

    let merge_key = merge_key.unwrap();
    let (mut patch_list, delete_list) =
        diff_lists_of_maps(original, modified, schema, merge_key, options)?;
    if !patch_list.is_empty() {
        patch_list = normalize_slice_order(&patch_list, modified, merge_key)?;
    }
    let order_same = is_order_same(original, modified, merge_key)?;
    let mut merged_patch_list = patch_list;
    if !delete_list.is_empty() {
        merged_patch_list.extend(delete_list);
    }

    let set_order = build_set_element_order_list_maps(
        modified,
        &merged_patch_list,
        order_same,
        merge_key,
        options,
    );

    if merged_patch_list.is_empty() {
        Ok(DiffListResult::new(None, None, set_order))
    } else {
        Ok(DiffListResult::new(
            Some(Value::Array(merged_patch_list)),
            None,
            set_order,
        ))
    }
}

struct DiffListResult {
    patch: Option<Value>,
    delete_list: Option<Value>,
    set_order: Option<Value>,
}

impl DiffListResult {
    fn new(patch: Option<Value>, delete_list: Option<Value>, set_order: Option<Value>) -> Self {
        Self {
            patch,
            delete_list,
            set_order,
        }
    }
}

fn build_set_element_order_list_maps(
    modified: &[Value],
    patch_list: &[Value],
    order_same: bool,
    merge_key: &str,
    options: &DiffOptions,
) -> Option<Value> {
    if !options.set_element_order {
        return None;
    }
    let generate = (!options.ignore_changes_and_additions
        && (!patch_list.is_empty() || !order_same))
        || (!options.ignore_deletions && !patch_list.is_empty());
    if !generate {
        return None;
    }
    let mut order_list = Vec::with_capacity(modified.len());
    for item in modified {
        if let Value::Object(obj) = item {
            if let Some(key_value) = obj.get(merge_key) {
                let mut entry = JsonMap::new();
                entry.insert(merge_key.to_string(), key_value.clone());
                order_list.push(Value::Object(entry));
            }
        }
    }
    if order_list.is_empty() {
        None
    } else {
        Some(Value::Array(order_list))
    }
}

fn build_set_element_order_list_scalars(
    original: &[Value],
    modified: &[Value],
    delete_list: &[Value],
    options: &DiffOptions,
) -> Option<Value> {
    if !options.set_element_order {
        return None;
    }
    let generate = (!options.ignore_deletions && !delete_list.is_empty())
        || (!options.ignore_changes_and_additions && original != modified);
    if !generate {
        return None;
    }
    Some(Value::Array(modified.to_vec()))
}

fn diff_lists_of_maps(
    original: &[Value],
    modified: &[Value],
    schema: &dyn LookupPatchMeta,
    merge_key: &str,
    options: &DiffOptions,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut patch_list = Vec::new();
    let mut delete_list = Vec::new();

    let original_sorted = sort_merge_lists_by_key(original, merge_key)?;
    let modified_sorted = sort_merge_lists_by_key(modified, merge_key)?;

    let mut original_index = 0usize;
    let mut modified_index = 0usize;
    loop {
        let original_in_bounds = original_index < original_sorted.len();
        let modified_in_bounds = modified_index < modified_sorted.len();
        if !original_in_bounds && !modified_in_bounds {
            break;
        }

        let (original_map, original_key_value) = if original_in_bounds {
            get_map_and_merge_key_value_by_index(original_index, merge_key, &original_sorted)?
        } else {
            (JsonMap::new(), Value::Null)
        };
        let (modified_map, modified_key_value) = if modified_in_bounds {
            get_map_and_merge_key_value_by_index(modified_index, merge_key, &modified_sorted)?
        } else {
            (JsonMap::new(), Value::Null)
        };
        let original_key_str = merge_key_value_string(&original_key_value, None);
        let modified_key_str = merge_key_value_string(&modified_key_value, None);

        match compare_list_values_at_index(
            original_in_bounds,
            modified_in_bounds,
            &original_key_str,
            &modified_key_str,
        ) {
            ListCompare::BothSame => {
                let child_patch = diff_maps(&original_map, &modified_map, schema, options)?;
                if !child_patch.is_empty() {
                    let mut patch_value = child_patch;
                    patch_value.insert(merge_key.to_string(), modified_key_value);
                    patch_list.push(Value::Object(patch_value));
                }
                original_index += 1;
                modified_index += 1;
            }
            ListCompare::OnlyModified => {
                if !options.ignore_changes_and_additions {
                    patch_list.push(Value::Object(modified_map));
                }
                modified_index += 1;
            }
            ListCompare::OnlyOriginal => {
                if !options.ignore_deletions {
                    let mut delete_item = JsonMap::new();
                    delete_item.insert(merge_key.to_string(), original_key_value);
                    delete_item.insert(
                        directive_keys::PATCH.to_string(),
                        Value::String("delete".to_string()),
                    );
                    delete_list.push(Value::Object(delete_item));
                }
                original_index += 1;
            }
        }
    }

    Ok((patch_list, delete_list))
}

fn diff_lists_of_scalars(
    original: &[Value],
    modified: &[Value],
    options: &DiffOptions,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut original_sorted = original.to_vec();
    let mut modified_sorted = modified.to_vec();
    original_sorted.sort_by(|a, b| scalar_cmp(a, b));
    modified_sorted.sort_by(|a, b| scalar_cmp(a, b));

    let mut add_list = Vec::new();
    let mut delete_list = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;

    loop {
        let original_in_bounds = i < original_sorted.len();
        let modified_in_bounds = j < modified_sorted.len();
        if !original_in_bounds && !modified_in_bounds {
            break;
        }

        let original_value = original_in_bounds.then(|| &original_sorted[i]);
        let modified_value = modified_in_bounds.then(|| &modified_sorted[j]);
        let original_string = original_value.map(scalar_string);
        let modified_string = modified_value.map(scalar_string);

        match compare_list_values_at_index(
            original_in_bounds,
            modified_in_bounds,
            original_string.as_deref().unwrap_or(""),
            modified_string.as_deref().unwrap_or(""),
        ) {
            ListCompare::BothSame => {
                i += 1;
                j += 1;
            }
            ListCompare::OnlyOriginal => {
                if !options.ignore_deletions {
                    if let Some(val) = original_value {
                        delete_list.push(val.clone());
                    }
                }
                i += 1;
            }
            ListCompare::OnlyModified => {
                if !options.ignore_changes_and_additions {
                    if let Some(val) = modified_value {
                        add_list.push(val.clone());
                    }
                }
                j += 1;
            }
        }
    }

    delete_list.sort_by(|a, b| scalar_cmp(a, b));
    delete_list.dedup();
    Ok((add_list, delete_list))
}

fn compare_list_values_at_index(
    list1_in_bounds: bool,
    list2_in_bounds: bool,
    list1_value: &str,
    list2_value: &str,
) -> ListCompare {
    let both_in_bounds = list1_in_bounds && list2_in_bounds;
    if both_in_bounds && list1_value == list2_value {
        return ListCompare::BothSame;
    }
    if !list1_in_bounds || (both_in_bounds && list1_value > list2_value) {
        return ListCompare::OnlyModified;
    }
    ListCompare::OnlyOriginal
}

#[derive(Clone, Copy, Debug)]
enum ListCompare {
    BothSame,
    OnlyOriginal,
    OnlyModified,
}

fn sort_merge_lists_by_key(list: &[Value], merge_key: &str) -> Result<Vec<Value>> {
    let mut sorted = list.to_vec();
    sorted.sort_by(|a, b| {
        let a_key = merge_key_value_string(a, Some(merge_key));
        let b_key = merge_key_value_string(b, Some(merge_key));
        a_key.cmp(&b_key)
    });
    Ok(sorted)
}

fn merge_key_value_string(value: &Value, merge_key: Option<&str>) -> String {
    if let Some(merge_key) = merge_key {
        return value
            .as_object()
            .and_then(|obj| obj.get(merge_key))
            .map(|v| merge_key_value_string(v, None))
            .unwrap_or_default();
    }
    match value {
        Value::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn get_map_and_merge_key_value_by_index(
    index: usize,
    merge_key: &str,
    list: &[Value],
) -> Result<(JsonMap, Value)> {
    let obj = list
        .get(index)
        .and_then(|v| v.as_object())
        .ok_or_else(|| Error::BadArgKind {
            expected: "object".to_string(),
            actual: value_kind(list.get(index).unwrap_or(&Value::Null)).to_string(),
        })?;
    let key_value = obj.get(merge_key).ok_or_else(|| Error::NoMergeKey {
        path: merge_key.to_string(),
        merge_key: merge_key.to_string(),
    })?;
    Ok((obj.clone(), key_value.clone()))
}

fn normalize_slice_order(
    to_sort: &[Value],
    order: &[Value],
    merge_key: &str,
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

fn index_in_order(order: &[Value], value: &Value, merge_key: &str) -> isize {
    for (i, item) in order.iter().enumerate() {
        if merge_key_value_string(item, Some(merge_key))
            == merge_key_value_string(value, Some(merge_key))
        {
            return i as isize;
        }
    }
    -1
}

fn is_order_same(original: &[Value], modified: &[Value], merge_key: &str) -> Result<bool> {
    if original.len() != modified.len() {
        return Ok(false);
    }
    for (orig_item, mod_item) in original.iter().zip(modified.iter()) {
        if merge_key_value_string(orig_item, Some(merge_key))
            != merge_key_value_string(mod_item, Some(merge_key))
        {
            return Ok(false);
        }
    }
    Ok(true)
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
    use crate::schema::EmptySchema;
    use serde_json::json;

    #[test]
    fn test_simple_map_diff_addition() {
        let original = json!({"a": 1}).as_object().unwrap().clone();
        let modified = json!({"a": 1, "b": 2}).as_object().unwrap().clone();
        let options = DiffOptions::default();
        let patch = diff_maps(&original, &modified, &EmptySchema, &options).expect("diff ok");
        let expected = json!({"b": 2}).as_object().unwrap().clone();
        assert_eq!(patch, expected);
    }
}
