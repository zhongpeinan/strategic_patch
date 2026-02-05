# JSON Merge Patch vs Strategic Merge Patch

## 概述

本文档详细对比两种 JSON 合并策略：**JSON Merge Patch** (RFC 7396) 和 **Strategic Merge Patch** (Kubernetes)。

---

## 1. 核心区别

### 1.1 列表合并行为（最关键差异）

| 维度 | JSON Merge Patch | Strategic Merge Patch |
|------|------------------|----------------------|
| **语义** | 数组是原子值 | 数组是可合并的集合 |
| **合并方式** | 整体替换 | 按 merge_key 递归合并或去重 |
| **精确控制** | 无法精确操作单个元素 | 支持增删改单个元素 |

#### 示例对比

```json
// 原始对象
{
  "containers": [
    {"name": "web", "image": "nginx:1.25"},
    {"name": "sidecar", "image": "busybox"}
  ]
}

// 补丁
{
  "containers": [
    {"name": "web", "image": "nginx:1.26"}
  ]
}

// JSON Merge Patch 结果
{
  "containers": [
    {"name": "web", "image": "nginx:1.26"}    // sidecar 被删除！
  ]
}

// Strategic Merge Patch 结果（按 name 合并）
{
  "containers": [
    {"name": "web", "image": "nginx:1.26"},    // 更新
    {"name": "sidecar", "image": "busybox"}     // 保留
  ]
}
```

### 1.2 字段删除语义

| 操作 | JSON Merge Patch | Strategic Merge Patch |
|------|------------------|----------------------|
| **删除对象字段** | 设置为 `null` | 设置为 `null` |
| **删除列表元素** | 无法精确删除 | 支持 `$patch: "delete"` |

#### Strategic Merge Patch 特有的列表元素删除

```json
{
  "containers": [
    {"$patch": "delete", "name": "old-sidecar"},
    {"name": "web", "image": "nginx:1.26"}
  ]
}
```

### 1.3 策略控制

| 特性 | JSON Merge Patch | Strategic Merge Patch |
|------|------------------|----------------------|
| **合并策略** | 固定 | 可配置（merge/replace/retainKeys） |
| **元数据** | 无 | 需要 Schema 定义 |
| **灵活性** | 低 | 高 |

#### Strategic Merge Patch 策略定义

```rust
#[derive(PatchSchema)]
struct PodSpec {
    #[patch(strategy = "merge", merge_key = "name")]
    containers: Vec<Container>,     // 按 name 合并

    #[patch(strategy = "replace")]
    volumes: Vec<Volume>,           // 整体替换

    #[patch(strategy = "retainKeys")]
    resources: ResourceRequirements, // 仅保留指定字段
}
```

### 1.4 特殊指令

**JSON Merge Patch：** 无特殊指令

**Strategic Merge Patch：** 支持多种指令

| 指令 | 位置 | 作用 |
|------|------|------|
| `$patch` | 对象/元素 | delete/replace/merge |
| `$retainKeys` | 对象 | 仅保留指定字段 |
| `$deleteFromPrimitiveList/field` | 并行 | 删除原始列表元素 |
| `$setElementOrder/field` | 并行 | 设置列表顺序 |

```json
{
  "$patch": "replace",
  "$retainKeys": ["name", "image"],
  "containers": [
    {"$patch": "delete", "name": "foo"},
    {"name": "web", "image": "nginx"}
  ],
  "$setElementOrder/containers": ["web", "sidecar"]
}
```

---

## 2. 联系与包含关系

```
┌─────────────────────────────────────────────────────────────┐
│              JSON Merge Patch (RFC 7396)                    │
│                                                              │
│  • 递归合并对象                                              │
│  • null 删除字段                                              │
│  • 数组整体替换                                              │
└─────────────────────────────────────────────────────────────┘
                           ↓ 扩展
┌─────────────────────────────────────────────────────────────┐
│         Strategic Merge Patch (Kubernetes)                  │
│                                                              │
│  • 继承 JSON Merge Patch 基础语义                            │
│  • 新增：数组按 merge_key 合并                               │
│  • 新增：策略控制（merge/replace/retainKeys）                │
│  • 新增：特殊指令（$patch, $retainKeys 等）                  │
│  • 需要：Schema 元数据                                        │
└─────────────────────────────────────────────────────────────┘
```

**关键关系：** Strategic Merge Patch 是 JSON Merge Patch 的**超集**，在没有特殊策略和 Schema 的情况下，行为退化为 JSON Merge Patch。

---

## 3. 算法层面对比

### 3.1 JSON Merge Patch 算法

```rust
fn json_merge_patch(original: &Value, patch: &Value) -> Value {
    match patch {
        // null → 删除
        Value::Null => Value::Null,

        // 对象 → 递归合并
        Value::Object(patch_map) => {
            match original {
                Value::Object(orig_map) => {
                    let mut result = orig_map.clone();
                    for (key, patch_val) in patch_map {
                        let orig_val = result.get(key);
                        result.insert(key, json_merge_patch(orig_val, patch_val));
                    }
                    Value::Object(result)
                }
                _ => patch.clone(),  // 类型不匹配，直接替换
            }
        }

        // 数组 → 整体替换
        Value::Array(_) => patch.clone(),

        // 原始值 → 直接替换
        _ => patch.clone(),
    }
}
```

### 3.2 Strategic Merge Patch 算法

```rust
fn strategic_merge_patch(
    original: &Value,
    patch: &Value,
    schema: &dyn LookupPatchMeta,
    key_path: &str,
) -> Value {
    match patch {
        // null → 删除
        Value::Null => Value::Null,

        // 对象 → 检查指令后递归合并
        Value::Object(patch_map) => {
            // 1. 处理 $patch 指令
            if let Some(directive) = patch_map.get("$patch") {
                return handle_patch_directive(directive, original);
            }

            // 2. 应用 $retainKeys
            let retain_keys = patch_map.get("$retainKeys");

            // 3. 递归合并
            match original {
                Value::Object(orig_map) => {
                    let mut result = orig_map.clone();
                    for (key, patch_val) in patch_map {
                        if is_directive_key(key) { continue; }

                        // 获取字段的合并策略
                        let (sub_schema, meta) = schema.lookup_struct_meta(key)?;

                        if meta.has_strategy(PatchStrategy::Replace) {
                            // Replace 策略：直接替换
                            result.insert(key, patch_val.clone());
                        } else {
                            // Merge 策略：递归合并
                            result.insert(
                                key,
                                strategic_merge_patch(
                                    orig_map.get(key),
                                    patch_val,
                                    &*sub_schema,
                                    &format!("{}.{}", key_path, key),
                                ),
                            );
                        }
                    }
                    Value::Object(result)
                }
                _ => patch.clone(),
            }
        }

        // 数组 → 根据策略处理
        Value::Array(patch_arr) => {
            match original {
                Value::Array(orig_arr) => {
                    let (sub_schema, meta) = schema.lookup_slice_meta(key_path)?;

                    if let Some(merge_key) = &meta.merge_key {
                        // 按 merge_key 合并
                        merge_lists_by_key(orig_arr, patch_arr, merge_key, &*sub_schema)
                    } else if meta.has_strategy(PatchStrategy::Replace) {
                        // Replace 策略：整体替换
                        patch_arr.clone()
                    } else {
                        // 原始数组：去重合并
                        merge_primitive_lists(orig_arr, patch_arr)
                    }
                }
                _ => patch.clone(),
            }
        }

        // 原始值 → 直接替换
        _ => patch.clone(),
    }
}
```

### 3.3 算法复杂度对比

| 操作 | JSON Merge Patch | Strategic Merge Patch |
|------|------------------|----------------------|
| 对象合并 | O(n) | O(n) |
| 数组合并 | O(1) | O(n×m) 或 O(n log n) |
| Schema 查找 | N/A | O(1) |
| 指令处理 | N/A | O(n) |

---

## 4. 应用场景对比

### 4.1 JSON Merge Patch 适用场景

**简单配置更新**

```rust
let config = json!({
    "database": {
        "host": "localhost",
        "port": 5432,
        "pool": {"max": 10, "min": 2}
    },
    "debug": false
});

let patch = json!({
    "database": {"host": "prod-db"},  // 更新嵌套字段
    "debug": true                      // 更新顶层字段
});

// 结果：递归合并，适合简单层级结构
```

**特点：**
- 无需预定义 Schema
- 行为可预测
- 适合通用配置

### 4.2 Strategic Merge Patch 适用场景

**Kubernetes PodSpec 更新**

```rust
let pod_spec = json!({
    "containers": [
        {"name": "web", "image": "nginx:1.25", "ports": [80]},
        {"name": "sidecar", "image": "busybox"}
    ]
});

let patch = json!({
    "containers": [
        {"name": "web", "image": "nginx:1.26"}  // 仅更新 web 的镜像
    ]
});

// Strategic Merge 结果：
// {
//     "containers": [
//         {"name": "web", "image": "nginx:1.26", "ports": [80]},  // 更新 + 保留
//         {"name": "sidecar", "image": "busybox"}                 // 不变
//     ]
// }
```

**声明式 API 状态管理**

```rust
// Server-Side Apply 场景
let original = load_from_etcd("pod-123");
let current_applied = get_applied_configuration();
let user_desired = get_user_request();

let patch = create_three_way_merge_patch(
    &original,
    &user_desired,
    &current_applied,
    &schema,
    false,  // 检测冲突
)?;
```

**特点：**
- 需要预定义 Schema
- 支持复杂的列表操作
- 适合 Kubernetes 类资源

---

## 5. 详细对比表

| 维度 | JSON Merge Patch | Strategic Merge Patch |
|------|------------------|----------------------|
| **标准化** | RFC 7396 | Kubernetes 专有 |
| **复杂度** | 简单 | 复杂（需要 Schema） |
| **列表处理** | 原子替换 | 智能合并 |
| **元素级操作** | 不支持 | 支持（通过指令） |
| **冲突检测** | 无 | 支持（three-way merge） |
| **类型安全** | 无 | 有（通过 Schema） |
| **表达能力** | 基础 | 丰富（策略 + 指令） |
| **适用领域** | 通用配置 | Kubernetes 资源 |
| **学习曲线** | 低 | 中 |
| **实现复杂度** | 低 | 高 |

---

## 6. 代码实现对比

### 6.1 完整的 JSON Merge Patch 实现

```rust
use serde_json::Value;

pub fn json_merge_patch(original: &Value, patch: &Value) -> Value {
    match patch {
        Value::Null => Value::Null,
        Value::Object(patch_map) => match original {
            Value::Object(orig_map) => {
                let mut result = serde_json::Map::new();
                // 保留原对象中未被 patch 覆盖的字段
                for (key, orig_val) in orig_map {
                    if !patch_map.contains_key(key) {
                        result.insert(key.clone(), orig_val.clone());
                    }
                }
                // 递归合并 patch 的字段
                for (key, patch_val) in patch_map {
                    let orig_val = orig_map.get(key);
                    result.insert(key.clone(), json_merge_patch(orig_val, patch_val));
                }
                Value::Object(result)
            }
            _ => patch.clone(),
        },
        Value::Array(patch_arr) => match original {
            Value::Array(_) => patch.clone(),
            _ => patch.clone(),
        },
        _ => patch.clone(),
    }
}
```

### 6.2 Strategic Merge Patch 列表合并核心

```rust
use std::collections::HashMap;

fn merge_lists_by_key(
    original: &[Value],
    patch: &[Value],
    merge_key: &str,
    schema: &dyn LookupPatchMeta,
) -> Result<Vec<Value>> {
    // 1. 构建原列表的索引
    let mut orig_by_key: HashMap<String, &Value> = HashMap::new();
    for item in original {
        if let Some(key) = extract_key(item, merge_key)? {
            orig_by_key.insert(key, item);
        }
    }

    let mut result = Vec::new();

    // 2. 处理 patch 中的特殊元素（删除/替换指令）
    for patch_item in patch {
        if let Some(directive) = patch_item.get("$patch") {
            match directive.as_str() {
                Some("delete") => {
                    if let Some(key) = extract_key(patch_item, merge_key)? {
                        orig_by_key.remove(&key);
                    }
                }
                Some("replace") => {
                    // 标记整体替换
                    return Ok(patch.to_vec());
                }
                _ => {}
            }
        }
    }

    // 3. 合并普通元素
    for patch_item in patch {
        if patch_item.get("$patch").is_some() {
            continue;
        }

        if let Some(key) = extract_key(patch_item, merge_key)? {
            if let Some(&orig_item) = orig_by_key.get(&key) {
                // 找到匹配项，递归合并
                let merged = strategic_merge_patch(
                    orig_item,
                    patch_item,
                    schema,
                    merge_key,
                )?;
                result.push(merged);
                orig_by_key.remove(&key);
            } else {
                // 新增项
                result.push(patch_item.clone());
            }
        }
    }

    // 4. 保留原列表中未被匹配的项
    for item in orig_by_key.values() {
        result.push((*item).clone());
    }

    Ok(result)
}
```

---

## 7. 选择建议

### 使用 JSON Merge Patch 当：

1. 配置结构简单，没有复杂的列表
2. 不需要预定义 Schema
3. 追求简单性和通用性
4. 列表整体替换行为可接受

### 使用 Strategic Merge Patch 当：

1. 需要精确操作列表中的单个元素
2. 工作在 Kubernetes 生态
3. 需要声明式 API 和冲突检测
4. 有明确的 Schema 定义
5. 需要保留未被修改的列表元素

---

## 8. 与 Kubernetes 的关系

Strategic Merge Patch 是 Kubernetes 为其声明式 API 专门设计的补丁格式：

```
Content-Type: application/strategic-merge-patch+json
```

它是 Kubernetes Server-Side Apply 的基础，解决了以下问题：

1. **多客户端协作**：不同控制器管理同一资源的不同字段
2. **冲突检测**：three-way merge 检测并发修改冲突
3. **部分更新**：精确控制资源中的单个元素
4. **字段保留**：未被管理的字段不受影响

---

## 9. 参考资源

- [RFC 7396 - JSON Merge Patch](https://datatracker.ietf.org/doc/html/rfc7396)
- [Kubernetes Strategic Merge Patch](https://kubernetes.io/docs/tasks/manage-kubernetes-objects/update-api-object-kubectl-patch/)
- [strategic_patch 实现设计](strategic_merge_patch_design.md)
