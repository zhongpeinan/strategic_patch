pub mod api;
pub mod directives;
pub mod error;
pub mod options;
pub mod schema;

mod conflict;
mod diff;
mod merge;
mod sort;
mod value_ext;

pub use api::*;
pub use error::{Error, Result};
pub use options::{DiffOptions, MergeOptions};
pub use schema::{
    EmptySchema, JsonArray, JsonMap, LookupPatchMeta, PatchMeta, PatchStrategy, PreconditionFn,
    StrategicPatchResource, schema_for,
};

#[cfg(feature = "derive")]
pub use strategic_patch_derive::PatchSchema;
