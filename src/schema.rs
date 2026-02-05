use crate::error::Result;

pub type JsonMap = serde_json::Map<String, serde_json::Value>;
pub type JsonArray = Vec<serde_json::Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatchStrategy {
    Merge,
    Replace,
    RetainKeys,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatchMeta {
    pub strategies: Vec<PatchStrategy>,
    pub merge_key: Option<String>,
}

impl PatchMeta {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn merge(key: impl Into<String>) -> Self {
        Self {
            strategies: vec![PatchStrategy::Merge],
            merge_key: Some(key.into()),
        }
    }

    pub fn replace() -> Self {
        Self {
            strategies: vec![PatchStrategy::Replace],
            merge_key: None,
        }
    }

    pub fn has_strategy(&self, strategy: PatchStrategy) -> bool {
        self.strategies.contains(&strategy)
    }

    pub fn extract_retain_keys(&self) -> (bool, Option<PatchStrategy>) {
        let has_retain = self.has_strategy(PatchStrategy::RetainKeys);
        let main = self
            .strategies
            .iter()
            .find(|&&s| s != PatchStrategy::RetainKeys)
            .copied();
        (has_retain, main)
    }
}

pub trait LookupPatchMeta: Send + Sync {
    fn lookup_struct_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)>;

    fn lookup_slice_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)>;

    fn name(&self) -> &str;

    fn has_field(&self, key: &str) -> bool {
        self.lookup_struct_meta(key).is_ok()
    }
}

pub type PreconditionFn = fn(&JsonMap) -> bool;

pub trait StrategicPatchResource: Sized {
    type Schema: LookupPatchMeta + Clone + 'static;

    fn schema() -> &'static Self::Schema;

    fn gvk() -> Option<&'static str> {
        None
    }

    fn preconditions() -> &'static [PreconditionFn] {
        &[]
    }
}

pub fn schema_for<T: StrategicPatchResource>() -> &'static dyn LookupPatchMeta {
    T::schema()
}

#[derive(Clone, Debug, Default)]
pub struct EmptySchema;

impl LookupPatchMeta for EmptySchema {
    fn lookup_struct_meta(&self, _key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn lookup_slice_meta(&self, _key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn name(&self) -> &str {
        "Empty"
    }
}
