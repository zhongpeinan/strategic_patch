pub mod directive_keys {
    pub const PATCH: &str = "$patch";
    pub const RETAIN_KEYS: &str = "$retainKeys";
    pub const DELETE_FROM_PRIMITIVE_LIST_PREFIX: &str = "$deleteFromPrimitiveList";
    pub const SET_ELEMENT_ORDER_PREFIX: &str = "$setElementOrder";
}

pub mod directive_values {
    pub const DELETE: &str = "delete";
    pub const REPLACE: &str = "replace";
    pub const MERGE: &str = "merge";
}

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::options::MergeOptions;
use crate::schema::JsonMap;

pub fn is_delete_list_key(key: &str) -> bool {
    key.starts_with(directive_keys::DELETE_FROM_PRIMITIVE_LIST_PREFIX)
}

pub fn is_set_order_key(key: &str) -> bool {
    key.starts_with(directive_keys::SET_ELEMENT_ORDER_PREFIX)
}

pub fn extract_field_from_directive<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    key.strip_prefix(prefix)?.strip_prefix('/')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchDirectiveAction {
    Delete,
    Replace,
}

pub fn handle_patch_directive(patch: &JsonMap) -> Result<Option<PatchDirectiveAction>> {
    let directive = match patch.get(directive_keys::PATCH) {
        Some(value) => value,
        None => return Ok(None),
    };
    let directive_str = directive.as_str().ok_or_else(|| {
        Error::BadPatchType(format!("expected string, got {directive:?}"))
    })?;
    match directive_str {
        directive_values::DELETE => Ok(Some(PatchDirectiveAction::Delete)),
        directive_values::REPLACE => Ok(Some(PatchDirectiveAction::Replace)),
        directive_values::MERGE => Ok(None),
        other => Err(Error::BadPatchType(other.to_string())),
    }
}

pub fn apply_retain_keys(
    original: &mut JsonMap,
    patch: &mut JsonMap,
    options: &MergeOptions,
) -> Result<()> {
    let Some(retain_value) = patch.remove(directive_keys::RETAIN_KEYS) else {
        return Ok(());
    };

    if !options.merge_parallel_list {
        if let Some(existing) = original.get(directive_keys::RETAIN_KEYS) {
            if existing != &retain_value {
                return Err(Error::BadPatchFormatForRetainKeys {
                    path: "".to_string(),
                });
            }
        } else {
            original.insert(directive_keys::RETAIN_KEYS.to_string(), retain_value);
        }
        return Ok(());
    }

    let retain_keys = retain_value.as_array().ok_or_else(|| {
        Error::BadPatchFormatForRetainKeys {
            path: "".to_string(),
        }
    })?;

    let keys_to_retain: HashSet<&str> = retain_keys
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    for (key, value) in patch.iter() {
        if key == directive_keys::PATCH
            || is_delete_list_key(key)
            || is_set_order_key(key)
        {
            continue;
        }
        if !value.is_null() && !keys_to_retain.contains(key.as_str()) {
            return Err(Error::BadPatchFormatForRetainKeys { path: key.clone() });
        }
    }

    original.retain(|k, _| keys_to_retain.contains(k.as_str()));

    Ok(())
}
