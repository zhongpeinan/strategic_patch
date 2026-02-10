#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};
use strategic_patch::{PatchSchema, StrategicPatchResource};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[patch(gvk = "apps/v1/Deployment")]
struct Deployment {
    name: String,
}

#[test]
fn test_gvk_returns_value() {
    assert_eq!(Deployment::gvk(), Some("apps/v1/Deployment"));
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
#[patch(gvk = "v1/Pod")]
struct Pod {
    name: String,
}

#[test]
fn test_gvk_pod() {
    assert_eq!(Pod::gvk(), Some("v1/Pod"));
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PatchSchema)]
struct NoGvk {
    name: String,
}

#[test]
fn test_no_gvk_returns_none() {
    assert_eq!(NoGvk::gvk(), None);
}
