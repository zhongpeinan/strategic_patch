#[derive(Clone, Debug, Default)]
pub struct DiffOptions {
    pub set_element_order: bool,
    pub ignore_changes_and_additions: bool,
    pub ignore_deletions: bool,
    pub build_retain_keys_directive: bool,
}

#[derive(Clone, Debug)]
pub struct MergeOptions {
    pub merge_parallel_list: bool,
    pub ignore_unmatched_nulls: bool,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            merge_parallel_list: true,
            ignore_unmatched_nulls: true,
        }
    }
}
