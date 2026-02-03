# Strategic Merge Patch 实施计划

> 本文档从设计文档中独立，用于跟踪实施进度。

---

## Phase 1: 基础框架

- [ ] 创建 `strategic_patch/` crate 结构
- [ ] 实现 `error.rs` (Error enum + http_status)
- [ ] 实现 `schema.rs` (PatchMeta + LookupPatchMeta + EmptySchema)
- [ ] 实现 `options.rs` (DiffOptions + MergeOptions)
- [ ] 实现 `directives.rs` (常量 + 检测函数)

## Phase 2: Derive 宏

- [ ] 创建 `strategic_patch_derive/` crate
- [ ] 实现 `#[derive(PatchSchema)]`
- [ ] 生成 Schema struct + static + impl
- [ ] 测试宏生成代码

## Phase 3: 核心算法

- [ ] 实现 `diff.rs` (diffMaps + diffLists)
- [ ] 实现 `merge.rs` (mergeMap + mergeSlice)
- [ ] 实现指令处理 ($patch, $retainKeys)
- [ ] 单元测试

## Phase 4: 列表操作

- [ ] 实现 list merge by mergeKey
- [ ] 实现 $deleteFromPrimitiveList
- [ ] 实现 $setElementOrder
- [ ] 实现 sort_merge_lists_by_name

## Phase 5: 高级功能

- [ ] 实现 three-way merge
- [ ] 实现 conflict detection
- [ ] 实现 merge multiple patches

## Phase 6: API 完善

- [ ] 实现 Layer 1 (Map API)
- [ ] 实现 Layer 2 (Bytes API)
- [ ] 实现 Layer 3 (Typed API)
- [ ] 实现 StrategicPatchResource trait

## Phase 7: 集成与文档

- [ ] 移植上游测试用例
- [ ] 集成测试
- [ ] 在 taibai_api 中集成使用
- [ ] API 文档
- [ ] 使用示例
