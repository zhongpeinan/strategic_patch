pub mod directives;
pub mod error;
pub mod options;
pub mod schema;
pub mod api;

mod conflict;
mod diff;
mod merge;
mod sort;
mod value_ext;

pub use error::{Error, Result};
pub use options::{DiffOptions, MergeOptions};
pub use schema::{
    schema_for, EmptySchema, JsonArray, JsonMap, LookupPatchMeta, PatchMeta, PatchStrategy,
    PreconditionFn, StrategicPatchResource,
};
pub use api::*;

#[cfg(feature = "derive")]
pub use strategic_patch_derive::PatchSchema;
