# Strategic Merge Patch 迁移设计 (v4)

> 目标：提供独立的 `strategic_patch` crate，与 Kubernetes `k8s.io/apimachinery/pkg/util/strategicpatch`
> 语义一致，兼顾上游行为精确对齐与 APIServer 使用体验。`taibai_api` 通过依赖集成。

---

## 1. 目标与范围

### 1.1 目标
- **语义一致**：完整覆盖 strategic patch 行为，与上游 Go 实现保持一致
- **易用性**：为 APIServer 提供直观的 API 和 schema 获取方式
- **类型安全**：提供强类型 API，减少运行时错误
- **可测试**：便于移植上游测试用例，确保兼容性

### 1.2 功能覆盖
| 函数 | 描述 |
|------|------|
| `create_two_way_merge_patch` | 从 original 和 modified 创建 patch |
| `strategic_merge_patch` | 应用 patch 到 original |
| `create_three_way_merge_patch` | 三路合并（server-side apply） |
| `merge_strategic_merge_map_patch` | 合并多个 patch |
| `merging_maps_have_conflicts` | 检测两个 patch 是否冲突 |
| `sort_merge_lists_by_name` | 测试辅助：按 mergeKey 排序 |

### 1.3 范围边界
- **不实现**：Server-Side Apply 字段管理器（ManagedFields）
- **无 schema 场景**：Unstructured / CRD 返回 `UnsupportedStrategicMergePatchFormat`
- **OpenAPI schema**：可选 feature（`strategicpatch-openapi`）

---

## 2. 上游行为对齐（关键点）

> 必须精确对齐 `patch.go` / `meta.go` / `types.go` / `errors.go` 的行为。

### 2.1 Patch 策略与 Merge Key

| Tag | 描述 | 示例 |
|-----|------|------|
| `x-kubernetes-patch-strategy: merge` | 列表按 mergeKey 合并 | `containers` |
| `x-kubernetes-patch-strategy: replace` | 整体替换 | `volumes` |
| `x-kubernetes-patch-strategy: retainKeys` | 只保留指定字段 | 与 merge/replace 可组合 |
| `x-kubernetes-patch-merge-key` | 列表元素的合并键 | `name`, `key` |

### 2.2 指令 (Directives)

```javascript
// 对象级指令
{"$patch": "delete"}              // 删除整个 map
{"$patch": "replace", ...}        // 替换整个 map

// 字段级指令
{"$retainKeys": ["field1", "field2"], "field1": "v1"}  // 只保留指定字段

// 列表级并行指令
{
  "containers": [...],                              // 主列表
  "$deleteFromPrimitiveList/args": ["--old-arg"],   // 删除标量元素
  "$setElementOrder/containers": [{"name": "a"}, {"name": "b"}]  // 顺序
}
```

### 2.3 Diff 行为 (CreateTwoWayMergePatch)

```
diffMaps(original, modified, schema, options):
  1. 遍历 modified:
     - 新增字段 -> patch[key] = modified[key]
     - 类型变化 -> patch[key] = modified[key]
     - map 类型 -> handleMapDiff (递归或 replace)
     - slice 类型 -> handleSliceDiff (merge 或 replace)
     - scalar 类型 -> 值不同则替换

  2. 处理删除:
     - original 有但 modified 无 -> patch[key] = null

  3. retainKeys:
     - 若 BuildRetainKeysDirective=true 且 modified[key]!=null -> 收集 key
     - 若 patch 非空或 original 有多余字段 -> 写入 $retainKeys
```

### 2.4 Merge 行为 (StrategicMergePatch)

```
mergeMap(original, patch, schema, options):
  1. 处理 $patch 指令:
     - "replace" -> 返回 patch (移除 $patch)
     - "delete" -> 返回 {}

  2. 处理 $retainKeys:
     - MergeParallelList=true -> 执行字段清理
     - MergeParallelList=false -> 仅合并 directive

  3. 处理 $setElementOrder:
     - MergeParallelList=true -> 验证并应用顺序
     - MergeParallelList=false -> 仅合并 directive

  4. 遍历 patch:
     - null 值 -> 删除字段 (IgnoreUnmatchedNulls 控制未匹配行为)
     - map -> mergeMapHandler
     - slice -> mergeSliceHandler
     - scalar -> 替换
```

### 2.5 Three-Way 行为 (CreateThreeWayMergePatch)

```
createThreeWayMergePatch(original, modified, current, schema, overwrite):
  1. delta = diff(current -> modified, IgnoreDeletions=true)
  2. deletions = diff(original -> modified, IgnoreChangesAndAdditions=true)
  3. patch = mergeMap(deletions, delta, MergeParallelList=false)
  4. if !overwrite:
       changed = diff(original -> current)
       if conflict(patch, changed) -> return ConflictError
  5. return patch
```

### 2.6 冲突检测 (MergingMapsHaveConflicts)

| 类型 | 冲突条件 |
|------|----------|
| map | `$patch` 指令不一致 |
| merge list (map 元素) | 按 mergeKey 构建 map-of-map 比较 |
| 非 merge list | 长度不同或任一元素冲突 |
| scalar | 值不一致 |

### 2.7 关键 Options

```rust
/// Diff 选项
pub struct DiffOptions {
    pub set_element_order: bool,           // 生成 $setElementOrder
    pub ignore_changes_and_additions: bool, // 只看删除 (3-way deletions)
    pub ignore_deletions: bool,             // 忽略删除 (3-way delta)
    pub build_retain_keys_directive: bool,  // 构建 $retainKeys
}

/// Merge 选项
pub struct MergeOptions {
    pub merge_parallel_list: bool,    // 应用并行列表指令
    pub ignore_unmatched_nulls: bool, // 忽略未匹配的 null
}
```

---

## 3. 架构设计

### 3.1 Crate 结构

strategicpatch 作为独立 crate，与 taibai_api 平级：

```
taibai/
├── strategic_patch/                # 核心 crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                  # crate 入口，re-export 公共 API
│       ├── api.rs                  # 公共 API (bytes/map/typed 三层)
│       ├── schema.rs               # LookupPatchMeta trait + PatchMeta
│       ├── diff.rs                 # diffMaps, diffLists
│       ├── merge.rs                # mergeMap, mergeSlice
│       ├── directives.rs           # $patch, $retainKeys, $setElementOrder 处理
│       ├── conflict.rs             # MergingMapsHaveConflicts
│       ├── sort.rs                 # sortMergeListsByName
│       ├── options.rs              # DiffOptions, MergeOptions
│       ├── error.rs                # Error enum
│       └── value_ext.rs            # serde_json::Value 扩展方法
│
├── strategic_patch_derive/         # proc-macro crate
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
│
└── taibai_api/                     # API 类型 crate (依赖 strategic_patch)
    ├── Cargo.toml
    └── src/
        └── ...
```

### 3.2 Crate 职责划分

| Crate | 职责 |
|-------|------|
| `strategic_patch` | 核心 patch 算法、API、Error、Options |
| `strategic_patch_derive` | `#[derive(PatchSchema)]` proc-macro |
| `taibai_api` | K8s API 类型定义，依赖 strategic_patch |

### 3.3 依赖关系

```
┌─────────────────────┐
│     taibai_api      │
│  (K8s API types)    │
└──────────┬──────────┘
           │ depends on
           ▼
┌─────────────────────┐     ┌─────────────────────────┐
│   strategic_patch   │◄────│ strategic_patch_derive  │
│   (core library)    │     │     (proc-macro)        │
└─────────────────────┘     └─────────────────────────┘
```

### 3.4 依赖配置

```toml
# strategic_patch/Cargo.toml
[package]
name = "strategic_patch"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
indexmap = "2.0"  # 保持 map 插入顺序

[dev-dependencies]
strategic_patch_derive = { path = "../strategic_patch_derive" }

[features]
default = []
openapi = []  # 可选：OpenAPI schema 支持
```

```toml
# strategic_patch_derive/Cargo.toml
[package]
name = "strategic_patch_derive"
version = "0.1.0"
edition = "2024"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2.0", features = ["full", "extra-traits"] }
quote = "1.0"
proc-macro2 = "1.0"
```

```toml
# taibai_api/Cargo.toml
[dependencies]
strategic_patch = { path = "../strategic_patch" }
strategic_patch_derive = { path = "../strategic_patch_derive" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
# ... 其他依赖
```

### 3.5 Re-export 策略

```rust
// strategic_patch/src/lib.rs

pub mod api;
pub mod schema;
pub mod error;
pub mod options;

mod diff;
mod merge;
mod directives;
mod conflict;
mod sort;
mod value_ext;

// 公共 API re-export
pub use api::*;
pub use schema::{LookupPatchMeta, PatchMeta, PatchStrategy, StrategicPatchResource};
pub use error::{Error, Result};
pub use options::{DiffOptions, MergeOptions};

// derive 宏 re-export (当 taibai_api 使用时)
#[cfg(feature = "derive")]
pub use strategic_patch_derive::PatchSchema;
```

```rust
// taibai_api 中使用
use strategic_patch::{
    strategic_merge_patch,
    StrategicPatchResource,
    PatchMeta,
    Error,
};
use strategic_patch_derive::PatchSchema;

#[derive(PatchSchema, Serialize, Deserialize)]
pub struct PodSpec {
    #[patch(strategy = "merge", merge_key = "name")]
    pub containers: Vec<Container>,
}
```

---

## 4. 数据模型

### 4.1 核心类型

```rust
// schema.rs

pub type JsonMap = serde_json::Map<String, serde_json::Value>;
pub type JsonArray = Vec<serde_json::Value>;

/// Patch 策略
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatchStrategy {
    Merge,
    Replace,
    RetainKeys,
}

/// 字段的 Patch 元数据
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

    pub fn has_strategy(&self, s: PatchStrategy) -> bool {
        self.strategies.contains(&s)
    }

    /// 提取 retainKeys 标志和主策略
    pub fn extract_retain_keys(&self) -> (bool, Option<PatchStrategy>) {
        let has_retain = self.has_strategy(PatchStrategy::RetainKeys);
        let main = self.strategies.iter()
            .find(|&&s| s != PatchStrategy::RetainKeys)
            .copied();
        (has_retain, main)
    }
}
```

### 4.2 LookupPatchMeta Trait

```rust
// schema.rs

use crate::strategicpatch::error::Result;

/// Schema 元数据查找 trait
///
/// 由 derive 宏为每个 API 类型自动实现。
pub trait LookupPatchMeta: Send + Sync {
    /// 查找 struct 字段的元数据
    fn lookup_struct_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)>;

    /// 查找 slice 字段的元数据（返回元素类型的 schema）
    fn lookup_slice_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)>;

    /// 类型名称（用于错误信息）
    fn name(&self) -> &str;

    /// 检查字段是否存在
    fn has_field(&self, key: &str) -> bool {
        self.lookup_struct_meta(key).is_ok()
    }
}

/// 空 schema（用于标量或无 schema 场景）
#[derive(Clone, Debug, Default)]
pub struct EmptySchema;

impl LookupPatchMeta for EmptySchema {
    fn lookup_struct_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn lookup_slice_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        Ok((Box::new(EmptySchema), PatchMeta::default()))
    }

    fn name(&self) -> &str {
        "Empty"
    }
}
```

### 4.3 错误类型

```rust
// error.rs

use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    #[error("invalid JSON document: {0}")]
    BadJsonDoc(String),

    #[error("invalid argument type: expected {expected}, got {actual}")]
    BadArgType { expected: String, actual: String },

    #[error("invalid argument kind: expected {expected}, got {actual}")]
    BadArgKind { expected: String, actual: String },

    #[error("merge key '{merge_key}' not found in map at {path}")]
    NoMergeKey { path: String, merge_key: String },

    #[error("lists of lists are not supported")]
    NoListOfLists,

    #[error("invalid $patch directive: {0}")]
    BadPatchType(String),

    #[error("invalid patch format for primitive list at {path}")]
    BadPatchFormatForPrimitiveList { path: String },

    #[error("invalid patch format for setElementOrder at {path}")]
    BadPatchFormatForSetElementOrderList { path: String },

    #[error("invalid patch format for retainKeys at {path}")]
    BadPatchFormatForRetainKeys { path: String },

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("conflict: patch={patch}, current={current}")]
    Conflict { patch: String, current: String },

    #[error("strategic merge patch not supported for this type")]
    UnsupportedStrategicMergePatchFormat,

    #[error("field '{field}' not found in '{path}'")]
    FieldNotFound { path: String, field: String },

    #[error("invalid type: expected {expected}, got {actual}")]
    InvalidType { expected: String, actual: String },

    #[error("inconsistent list element types")]
    InconsistentListElementTypes,

    #[error("JSON error: {0}")]
    Json(String),
}

impl Error {
    /// 映射到 HTTP 状态码
    pub fn http_status(&self) -> u16 {
        match self {
            Error::BadJsonDoc(_)
            | Error::BadArgType { .. }
            | Error::BadArgKind { .. }
            | Error::BadPatchType(_)
            | Error::BadPatchFormatForPrimitiveList { .. }
            | Error::BadPatchFormatForSetElementOrderList { .. }
            | Error::BadPatchFormatForRetainKeys { .. }
            | Error::PreconditionFailed(_)
            | Error::Json(_) => 400, // Bad Request

            Error::NoMergeKey { .. }
            | Error::NoListOfLists
            | Error::FieldNotFound { .. }
            | Error::InvalidType { .. }
            | Error::InconsistentListElementTypes => 422, // Unprocessable Entity

            Error::UnsupportedStrategicMergePatchFormat => 415, // Unsupported Media Type

            Error::Conflict { .. } => 409, // Conflict
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
```

---

## 5. 公共 API 设计

### 5.1 三层 API 架构

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 3: Typed API (泛型，自动序列化/反序列化)              │
│  strategic_merge_patch_typed::<Pod>(original, patch)        │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: Bytes API (原始字节，兼容上游签名)                 │
│  strategic_merge_patch(original: &[u8], patch: &[u8], ...)  │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Map API (核心实现，操作 JsonMap)                   │
│  strategic_merge_map_patch(original: &JsonMap, patch, ...)  │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 Layer 1: Map API（核心）

```rust
// api.rs

/// 创建两路合并 patch (Map 版)
pub fn create_two_way_merge_map_patch(
    original: &JsonMap,
    modified: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<JsonMap>;

/// 创建两路合并 patch (带前置条件)
pub fn create_two_way_merge_map_patch_with_preconditions(
    original: &JsonMap,
    modified: &JsonMap,
    schema: &dyn LookupPatchMeta,
    preconditions: &[PreconditionFn],
) -> Result<JsonMap>;

/// 应用 strategic merge patch (Map 版)
pub fn strategic_merge_map_patch(
    original: &JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<JsonMap>;

/// 应用 strategic merge patch (带选项)
pub fn strategic_merge_map_patch_with_options(
    original: &JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &MergeOptions,
) -> Result<JsonMap>;

/// 创建三路合并 patch (Map 版)
pub fn create_three_way_merge_map_patch(
    original: &JsonMap,
    modified: &JsonMap,
    current: &JsonMap,
    schema: &dyn LookupPatchMeta,
    overwrite: bool,
) -> Result<JsonMap>;

/// 合并多个 patch
pub fn merge_strategic_merge_map_patch(
    schema: &dyn LookupPatchMeta,
    patches: &[&JsonMap],
) -> Result<JsonMap>;

/// 检测冲突
pub fn merging_maps_have_conflicts(
    left: &JsonMap,
    right: &JsonMap,
    schema: &dyn LookupPatchMeta,
) -> Result<bool>;

/// 前置条件函数类型
pub type PreconditionFn = fn(&JsonMap) -> bool;
```

### 5.3 Layer 2: Bytes API

```rust
// api.rs

/// 创建两路合并 patch (Bytes 版)
pub fn create_two_way_merge_patch(
    original: &[u8],
    modified: &[u8],
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)?;
    let modi: JsonMap = serde_json::from_slice(modified)?;
    let patch = create_two_way_merge_map_patch(&orig, &modi, schema)?;
    Ok(serde_json::to_vec(&patch)?)
}

/// 应用 strategic merge patch (Bytes 版)
pub fn strategic_merge_patch(
    original: &[u8],
    patch: &[u8],
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)?;
    let p: JsonMap = serde_json::from_slice(patch)?;
    let result = strategic_merge_map_patch(&orig, &p, schema)?;
    Ok(serde_json::to_vec(&result)?)
}

/// 创建三路合并 patch (Bytes 版)
pub fn create_three_way_merge_patch(
    original: &[u8],
    modified: &[u8],
    current: &[u8],
    schema: &dyn LookupPatchMeta,
    overwrite: bool,
) -> Result<Vec<u8>> {
    let orig: JsonMap = serde_json::from_slice(original)?;
    let modi: JsonMap = serde_json::from_slice(modified)?;
    let curr: JsonMap = serde_json::from_slice(current)?;
    let patch = create_three_way_merge_map_patch(&orig, &modi, &curr, schema, overwrite)?;
    Ok(serde_json::to_vec(&patch)?)
}

/// 按 mergeKey 排序 (测试辅助)
pub fn sort_merge_lists_by_name(
    value: &[u8],
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<u8>>;
```

### 5.4 Layer 3: Typed API

```rust
// api.rs

use serde::{de::DeserializeOwned, Serialize};

/// 强类型 strategic merge patch
///
/// 自动处理序列化/反序列化，适合 APIServer 直接使用。
pub fn strategic_merge_patch_typed<T>(
    original: &T,
    patch: &[u8],
) -> Result<T>
where
    T: StrategicPatchResource + Serialize + DeserializeOwned,
{
    let orig_bytes = serde_json::to_vec(original)?;
    let result_bytes = strategic_merge_patch(&orig_bytes, patch, T::schema())?;
    Ok(serde_json::from_slice(&result_bytes)?)
}

/// 强类型三路合并
pub fn create_three_way_merge_patch_typed<T>(
    original: &T,
    modified: &T,
    current: &T,
    overwrite: bool,
) -> Result<Vec<u8>>
where
    T: StrategicPatchResource + Serialize,
{
    let orig = serde_json::to_vec(original)?;
    let modi = serde_json::to_vec(modified)?;
    let curr = serde_json::to_vec(current)?;
    create_three_way_merge_patch(&orig, &modi, &curr, T::schema(), overwrite)
}
```

---

## 6. 易用性增强

### 6.1 StrategicPatchResource Trait

```rust
// schema.rs

/// 支持 Strategic Merge Patch 的资源 trait
///
/// 由 derive 宏自动为 API 类型实现。
pub trait StrategicPatchResource: Sized {
    /// Schema 类型
    type Schema: LookupPatchMeta + 'static;

    /// 获取静态 schema 引用
    fn schema() -> &'static Self::Schema;

    /// 获取 GVK (可选，用于日志/错误)
    fn gvk() -> Option<&'static str> {
        None
    }

    /// 前置条件 (可选)
    fn preconditions() -> &'static [PreconditionFn] {
        &[]
    }
}
```

### 6.2 Derive 宏生成

```rust
// strategic_patch_derive/src/lib.rs

/// 为 struct 生成 PatchSchema 实现
///
/// # 属性
/// - `#[patch(strategy = "merge", merge_key = "name")]` - 字段级
/// - `#[patch(strategy = "replace")]` - 字段级
/// - `#[patch(strategy = "merge,retainKeys", merge_key = "key")]` - 组合
///
/// # 示例
/// ```rust
/// #[derive(PatchSchema)]
/// struct PodSpec {
///     #[patch(strategy = "merge", merge_key = "name")]
///     containers: Vec<Container>,
///
///     #[patch(strategy = "replace")]
///     volumes: Option<Vec<Volume>>,
///
///     restart_policy: Option<String>,  // 默认：无特殊策略
/// }
/// ```
#[proc_macro_derive(PatchSchema, attributes(patch))]
pub fn derive_patch_schema(input: TokenStream) -> TokenStream {
    // 生成:
    // 1. XxxSchema struct
    // 2. static XXX_SCHEMA: XxxSchema = XxxSchema;
    // 3. impl LookupPatchMeta for XxxSchema
    // 4. impl StrategicPatchResource for Xxx
    // 5. impl Xxx { pub fn schema() -> &'static XxxSchema }
}
```

### 6.3 生成代码示例

```rust
// 用户代码
#[derive(PatchSchema, Serialize, Deserialize)]
struct PodSpec {
    #[patch(strategy = "merge", merge_key = "name")]
    containers: Vec<Container>,
}

// 生成代码
pub struct PodSpecSchema;

static POD_SPEC_SCHEMA: PodSpecSchema = PodSpecSchema;

impl LookupPatchMeta for PodSpecSchema {
    fn lookup_struct_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        match key {
            "containers" => Ok((
                Box::new(ContainerSchema),
                PatchMeta {
                    strategies: vec![PatchStrategy::Merge],
                    merge_key: Some("name".into()),
                },
            )),
            _ => Ok((Box::new(EmptySchema), PatchMeta::default())),
        }
    }

    fn lookup_slice_meta(&self, key: &str) -> Result<(Box<dyn LookupPatchMeta>, PatchMeta)> {
        self.lookup_struct_meta(key)
    }

    fn name(&self) -> &str {
        "PodSpec"
    }
}

impl StrategicPatchResource for PodSpec {
    type Schema = PodSpecSchema;

    fn schema() -> &'static Self::Schema {
        &POD_SPEC_SCHEMA
    }
}

impl PodSpec {
    pub fn schema() -> &'static PodSpecSchema {
        &POD_SPEC_SCHEMA
    }
}
```

### 6.4 泛型辅助函数

```rust
// schema.rs

/// 获取类型的 schema
pub fn schema_for<T: StrategicPatchResource>() -> &'static dyn LookupPatchMeta {
    T::schema()
}
```

---

## 7. APIServer 集成

### 7.1 PATCH 入口流程

```
┌──────────────────────────────────────────────────────────────┐
│ 1. Content-Type 判定                                         │
│    application/strategic-merge-patch+json -> strategicpatch  │
│    application/json-patch+json -> jsonpatch                  │
│    application/merge-patch+json -> mergepatch                │
├──────────────────────────────────────────────────────────────┤
│ 2. 读取输入                                                   │
│    patch_bytes = request.body()                              │
│    current = storage.get(key)                                │
├──────────────────────────────────────────────────────────────┤
│ 3. 获取 Schema                                                │
│    schema = R::schema()  // 通过泛型                          │
│    或 registry.get(gvk)  // 通过注册表                        │
├──────────────────────────────────────────────────────────────┤
│ 4. 应用 Patch                                                 │
│    merged = strategic_merge_patch_typed::<R>(&current, patch) │
├──────────────────────────────────────────────────────────────┤
│ 5. 校验 + Admission                                           │
│    validate(&merged)?                                         │
│    apply_defaults(&mut merged)?                               │
│    admission_mutate(&mut merged)?                             │
├──────────────────────────────────────────────────────────────┤
│ 6. 写入存储                                                   │
│    storage.put(key, &merged)?                                 │
│    return Ok(merged)                                          │
└──────────────────────────────────────────────────────────────┘
```

### 7.2 泛型 Handler 示例

```rust
use strategic_patch::{
    strategic_merge_patch_typed, StrategicPatchResource, Error,
};

async fn handle_strategic_patch<R>(
    current: R,
    patch_bytes: &[u8],
) -> Result<R, ApiError>
where
    R: StrategicPatchResource + Serialize + DeserializeOwned + Validate,
{
    // 应用 patch
    let mut merged = strategic_merge_patch_typed::<R>(&current, patch_bytes)
        .map_err(|e| ApiError::new(e.http_status(), e.to_string()))?;

    // 校验
    merged.validate()?;

    Ok(merged)
}
```

### 7.3 SchemaRegistry（可选）

```rust
// 适用于动态资源类型
pub struct SchemaRegistry {
    schemas: HashMap<String, &'static dyn LookupPatchMeta>,
}

impl SchemaRegistry {
    pub fn register<R: StrategicPatchResource>(&mut self, gvk: &str) {
        self.schemas.insert(gvk.to_string(), R::schema());
    }

    pub fn get(&self, gvk: &str) -> Option<&'static dyn LookupPatchMeta> {
        self.schemas.get(gvk).copied()
    }
}
```

---

## 8. 指令处理详细设计

### 8.1 指令常量

```rust
// directives.rs

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
```

### 8.2 指令检测

```rust
// directives.rs

/// 检查是否为 $deleteFromPrimitiveList 指令键
pub fn is_delete_list_key(key: &str) -> bool {
    key.starts_with(directive_keys::DELETE_FROM_PRIMITIVE_LIST_PREFIX)
}

/// 检查是否为 $setElementOrder 指令键
pub fn is_set_order_key(key: &str) -> bool {
    key.starts_with(directive_keys::SET_ELEMENT_ORDER_PREFIX)
}

/// 从指令键提取字段名
/// "$deleteFromPrimitiveList/args" -> "args"
pub fn extract_field_from_directive(key: &str, prefix: &str) -> Option<&str> {
    key.strip_prefix(prefix)?.strip_prefix('/')
}
```

### 8.3 $patch 指令处理

```rust
// directives.rs

/// 处理 $patch 指令
pub fn handle_patch_directive(
    patch: &JsonMap,
) -> Result<Option<PatchDirectiveAction>> {
    let directive = match patch.get(directive_keys::PATCH) {
        Some(v) => v,
        None => return Ok(None),
    };

    let directive_str = directive.as_str().ok_or_else(|| {
        Error::BadPatchType(format!("expected string, got {:?}", directive))
    })?;

    match directive_str {
        directive_values::DELETE => Ok(Some(PatchDirectiveAction::Delete)),
        directive_values::REPLACE => Ok(Some(PatchDirectiveAction::Replace)),
        directive_values::MERGE => Ok(None), // merge 不需要特殊处理
        other => Err(Error::BadPatchType(other.to_string())),
    }
}

pub enum PatchDirectiveAction {
    Delete,
    Replace,
}
```

### 8.4 $retainKeys 指令处理

```rust
// directives.rs

/// 应用 $retainKeys 指令
pub fn apply_retain_keys(
    original: &mut JsonMap,
    patch: &JsonMap,
    options: &MergeOptions,
) -> Result<()> {
    let retain_keys = match patch.get(directive_keys::RETAIN_KEYS) {
        Some(Value::Array(arr)) => arr,
        Some(_) => return Err(Error::BadPatchFormatForRetainKeys { path: "".into() }),
        None => return Ok(()),
    };

    // 收集要保留的 key
    let keys_to_retain: HashSet<&str> = retain_keys
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // 验证 patch 中的非 null 字段都在 retain list 中
    for (key, value) in patch {
        if key == directive_keys::RETAIN_KEYS || key == directive_keys::PATCH {
            continue;
        }
        if !value.is_null() && !keys_to_retain.contains(key.as_str()) {
            return Err(Error::BadPatchFormatForRetainKeys { path: key.clone() });
        }
    }

    // 如果 MergeParallelList=true，执行清理
    if options.merge_parallel_list {
        original.retain(|k, _| keys_to_retain.contains(k.as_str()));
    }

    Ok(())
}
```

---

## 9. 测试策略

### 9.1 测试层次

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 4: 上游兼容测试                                        │
│ - 移植 patch_test.go 关键用例                                │
│ - JSON fixture 对比                                          │
├─────────────────────────────────────────────────────────────┤
│ Layer 3: 集成测试                                            │
│ - two-way / three-way 完整流程                               │
│ - 多个 patch 合并                                            │
├─────────────────────────────────────────────────────────────┤
│ Layer 2: 模块测试                                            │
│ - diff/merge/conflict 各模块                                 │
│ - 指令处理                                                   │
├─────────────────────────────────────────────────────────────┤
│ Layer 1: 单元测试                                            │
│ - PatchMeta / PatchStrategy                                  │
│ - Error 类型                                                 │
│ - 辅助函数                                                   │
└─────────────────────────────────────────────────────────────┘
```

### 9.2 关键测试用例

```rust
#[cfg(test)]
mod tests {
    // 基础 map merge
    #[test]
    fn test_simple_map_merge() { ... }

    // list merge with mergeKey
    #[test]
    fn test_list_merge_by_key() { ... }

    // $patch: delete
    #[test]
    fn test_patch_delete_directive() { ... }

    // $patch: replace
    #[test]
    fn test_patch_replace_directive() { ... }

    // $retainKeys
    #[test]
    fn test_retain_keys() { ... }

    // $deleteFromPrimitiveList
    #[test]
    fn test_delete_from_primitive_list() { ... }

    // $setElementOrder
    #[test]
    fn test_set_element_order() { ... }

    // three-way merge
    #[test]
    fn test_three_way_merge_no_conflict() { ... }

    // three-way conflict
    #[test]
    fn test_three_way_merge_conflict() { ... }

    // list of lists error
    #[test]
    fn test_list_of_lists_error() { ... }

    // missing mergeKey error
    #[test]
    fn test_missing_merge_key_error() { ... }

    // IgnoreUnmatchedNulls
    #[test]
    fn test_ignore_unmatched_nulls() { ... }
}
```

### 9.3 上游测试移植

```rust
// tests/upstream_compat_test.rs

/// 从 patch_test.go 移植的测试用例
/// 使用 JSON fixture 文件，确保输出一致

#[test]
fn test_upstream_two_way_cases() {
    let cases = load_fixtures("testdata/two_way_cases.json");
    for case in cases {
        let result = create_two_way_merge_map_patch(
            &case.original,
            &case.modified,
            &case.schema,
        );
        assert_json_eq!(result.unwrap(), case.expected);
    }
}
```

---

## 10. 性能考虑

### 10.1 避免过度 Clone

```rust
// 使用引用和 Cow 减少拷贝
pub fn merge_map_in_place(
    original: &mut JsonMap,
    patch: &JsonMap,
    schema: &dyn LookupPatchMeta,
    options: &MergeOptions,
) -> Result<()>;

// 内部实现使用 take/insert 而非 clone
fn merge_field(original: &mut JsonMap, key: &str, patch_value: Value) {
    original.insert(key.to_string(), patch_value);
}
```

### 10.2 排序与去重

```rust
// 使用 IndexMap 保持插入顺序
use indexmap::IndexMap;

// 列表去重使用 HashSet
fn dedup_scalars(list: &mut Vec<Value>) {
    let mut seen = HashSet::new();
    list.retain(|v| {
        let key = format!("{:?}", v);
        seen.insert(key)
    });
}
```

### 10.3 Schema 查找缓存

```rust
// Schema 是静态的，无需缓存
// LookupPatchMeta 实现应当是 O(1) 的 match 分支
```

---

## 11. 实施计划

### Phase 1: 基础框架
- [ ] 创建 `strategic_patch/` crate 结构
- [ ] 实现 `error.rs` (Error enum + http_status)
- [ ] 实现 `schema.rs` (PatchMeta + LookupPatchMeta + EmptySchema)
- [ ] 实现 `options.rs` (DiffOptions + MergeOptions)
- [ ] 实现 `directives.rs` (常量 + 检测函数)

### Phase 2: Derive 宏
- [ ] 创建 `strategic_patch_derive/` crate
- [ ] 实现 `#[derive(PatchSchema)]`
- [ ] 生成 Schema struct + static + impl
- [ ] 测试宏生成代码

### Phase 3: 核心算法
- [ ] 实现 `diff.rs` (diffMaps + diffLists)
- [ ] 实现 `merge.rs` (mergeMap + mergeSlice)
- [ ] 实现指令处理 ($patch, $retainKeys)
- [ ] 单元测试

### Phase 4: 列表操作
- [ ] 实现 list merge by mergeKey
- [ ] 实现 $deleteFromPrimitiveList
- [ ] 实现 $setElementOrder
- [ ] 实现 sort_merge_lists_by_name

### Phase 5: 高级功能
- [ ] 实现 three-way merge
- [ ] 实现 conflict detection
- [ ] 实现 merge multiple patches

### Phase 6: API 完善
- [ ] 实现 Layer 1 (Map API)
- [ ] 实现 Layer 2 (Bytes API)
- [ ] 实现 Layer 3 (Typed API)
- [ ] 实现 StrategicPatchResource trait

### Phase 7: 集成与文档
- [ ] 移植上游测试用例
- [ ] 集成测试
- [ ] 在 taibai_api 中集成使用
- [ ] API 文档
- [ ] 使用示例

---

## 12. 开放问题

1. **OpenAPI Schema 支持**：是否作为默认 feature 还是可选？
2. **CRD 支持**：无 schema 时是否支持 fallback 到 JSON Merge Patch？
3. **性能基准**：是否需要与 Go 实现做性能对比？
4. **serde_json 精度**：大整数是否需要特殊处理？

---

## 13. 附录：与 v1/v2/v3 的改进点

| 方面 | v1 | v2 | v3 | v4 |
|------|----|----|----|----|
| Crate 结构 | 子模块 | 子模块 | 子模块 | **独立 crate** |
| 上游对齐 | 一般 | 详细 | 引用v2 | 保留v2详细内容 |
| 代码实现 | 完整但粗糙 | 无 | 无 | 设计+关键实现 |
| API 层次 | 单层 | bytes+map | +typed | 三层架构明确 |
| Schema 获取 | derive | derive+静态 | +Resource trait | 完整设计 |
| 错误处理 | 基础 | 完整 | +http_status | 保留+增强 |
| 测试策略 | 有 | 有 | 简略 | 分层详细 |
| 实施计划 | 按周 | 按步骤 | 无 | 按Phase |
