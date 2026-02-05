# Create Patch 设计文档（Two-Way / Three-Way）

> 目标：补充“创建 patch”部分的设计，覆盖 two-way 与 three-way 生成逻辑，
> 与 upstream `k8s.io/apimachinery/pkg/util/strategicpatch` 行为一致。

---

## 1. 范围

### 1.1 覆盖函数
- `create_two_way_merge_map_patch`
- `create_two_way_merge_map_patch_with_preconditions`
- `create_two_way_merge_patch`（Bytes）
- `create_three_way_merge_map_patch`
- `create_three_way_merge_patch`（Bytes）
- `create_three_way_merge_patch_typed`

### 1.2 不覆盖
- JSON Merge Patch（non-strategic）
- Server-Side Apply ManagedFields
- OpenAPI schema （feature 预留）

---

## 2. 行为基准（Upstream 对齐）

### 2.1 Two-Way (CreateTwoWayMergePatch)

```
diffMaps(original, modified, schema, options):
  1) 遍历 modified
     - original 无 -> patch[key] = modified[key]
     - 类型变更 -> patch[key] = modified[key]
     - map -> 递归 diffMaps (replace 策略直接替换)
     - list -> diffLists (merge / replace)
     - scalar -> 值不同则替换

  2) 删除逻辑（IgnoreDeletions=false）
     - original 有但 modified 无 -> patch[key] = null

  3) retainKeys
     - options.build_retain_keys_directive=true
     - modified[key] != null -> 收集 key
     - patch 非空或 original 有额外字段 -> 写入 $retainKeys
```

#### 默认选项
- Two-way 默认 `set_element_order = true`，与 upstream 行为一致。

#### DiffOptions 语义
```
set_element_order           // 生成 $setElementOrder
ignore_changes_and_additions// 只计算删除 (three-way deletions)
ignore_deletions            // 忽略删除 (three-way delta)
build_retain_keys_directive // 生成 $retainKeys
```

### 2.2 Three-Way (CreateThreeWayMergePatch)

```
createThreeWayMergePatch(original, modified, current, schema, overwrite):
  1) delta = diff(current -> modified, IgnoreDeletions=true)
  2) deletions = diff(original -> modified, IgnoreChangesAndAdditions=true)
  3) patch = mergeMap(deletions, delta, MergeParallelList=false)
  4) if !overwrite:
       changed = diff(original -> current)
       if conflict(patch, changed) -> ConflictError
  5) return patch
```

---

## 3. 数据模型

### 3.1 输入/输出
- 输入：`JsonMap` (`serde_json::Map<String, Value>`)
- 输出：`JsonMap`
- bytes 层：`serde_json::from_slice` / `serde_json::to_vec`

### 3.2 Schema
- 使用 `LookupPatchMeta` 查找字段策略和 mergeKey
- 若无 schema（或 EmptySchema）：
  - list 无 mergeKey -> replace 行为
  - map 递归处理但无特殊策略

---

## 4. 核心算法设计

### 4.1 diffMaps

关键路径：
- 字段新增：`patch[key] = modified[key]`
- 类型变化：直接替换
- map：递归（Replace 策略直接替换）
- list：
  - mergeKey 存在 -> merge diff by key
  - mergeKey 缺失 -> replace
- scalar：值不同替换
- 删除：`patch[key] = null`（ignore_deletions=false）
- retainKeys：生成 `$retainKeys`
- directive marker：若 `$patch` 字段值发生变化，写入 patch（上游 `handleDirectiveMarker`）

### 4.2 diffLists (mergeKey)

- 构建 `merge_key -> object` 索引
- 新增：加入 patch list
- 修改：递归 diff，生成 partial map
- 删除：生成 `{"$patch":"delete", mergeKey: ...}`
- `set_element_order`：生成 `$setElementOrder/<field>`（map list：mergeKey 映射；primitive list：完整列表）
- **primitive list 删除**：生成 `$deleteFromPrimitiveList/<field>` 并行删除列表
- **排序行为**：patch list 会按 mergeKey 归一化排序，删除项追加到末尾

### 4.3 mergeMap (用于 three-way 组合)

在 three-way 中 `MergeParallelList=false`：
- 仅合并 directive，不应用 list 排序 / deleteFromPrimitiveList
- 保持 patch 结构用于冲突检测

---

## 5. 错误与前置条件

### 5.1 Preconditions
`create_two_way_merge_map_patch_with_preconditions`：
- 每个 `PreconditionFn` **对生成的 patch** 进行校验（与 upstream 一致）
- 任一失败 -> `Error::PreconditionFailed`

### 5.2 关键错误
- `NoMergeKey`：merge list 元素缺少 mergeKey
- `NoListOfLists`：list of list 不支持
- `BadArgType` / `BadArgKind`：输入类型不匹配
- `Conflict`：three-way 冲突

---

## 6. API 设计

### 6.1 Map API
```
create_two_way_merge_map_patch
create_two_way_merge_map_patch_with_preconditions
create_three_way_merge_map_patch
```

### 6.2 Bytes API
```
create_two_way_merge_patch
create_three_way_merge_patch
```

### 6.3 Typed API
```
create_three_way_merge_patch_typed
```

---

## 7. 测试策略（对应 upstream）

优先移植：
- TwoWay / ThreeWay 流程
- retainKeys
- mergeKey list 的 add/update/delete
- setElementOrder 生成与合并
- conflict 检测

测试来源：
`k8s/staging/src/k8s.io/apimachinery/pkg/util/strategicpatch/patch_test.go`

---

## 8. 与现有实现的映射

| 设计模块 | 代码位置 |
|----------|----------|
| diffMaps / diffLists | `strategic_patch/src/diff.rs` |
| create two-way | `strategic_patch/src/api.rs` |
| create three-way | `strategic_patch/src/api.rs` |
| conflict 检测 | `strategic_patch/src/conflict.rs` |
| mergeMap | `strategic_patch/src/merge.rs` |

---

## 9. 兼容性注意事项

- retainKeys 列表在 upstream 中会排序（`sortScalars`），Rust 实现应保持一致。
- Apply 阶段对 Unstructured/无 schema 的资源应返回 Unsupported（行为与 upstream apply 一致）。
