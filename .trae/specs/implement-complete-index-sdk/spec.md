# 完整对齐 Index 核心能力 Spec

## Why
当前项目的 Rust core 与 TypeScript SDK 只搭好了类名和基础结构，`addDocuments`、`search`、`updateSettings` 以及任务返回仍是占位实现，无法体现 README 中“尽量对齐官方 `meilisearch-js`”的目标。用户明确要求走完整路线，因此第一阶段应优先补齐可用且可验证的 Index 核心能力，而不是继续保留临时直连或伪任务结果。

## What Changes
- 在 `packages/core` 中接入完整的索引写入与任务执行路径，优先以 vendored Meilisearch 的 `index-scheduler` / 上游任务模型为基准组织写入、设置更新与任务状态。
- 在 `packages/core` 中实现真实可用的 `search` 执行路径，返回稳定的搜索结果结构，而不是抛出“未实现”错误。
- 在 `packages/core` 中暴露任务查询与等待能力，支撑 TypeScript SDK 返回与官方 `meilisearch-js` 接近的 task 对象与等待行为。
- 在 `packages/api` 中重构 `Client`、`Engine`、`Index` 的方法与类型，优先对齐官方 SDK 的方法命名、参数形状、错误形状与常用返回值。
- 在 `packages/api` 中补充面向用户的测试，验证从索引创建到文档写入、搜索、设置更新、任务等待的主路径行为。
- **BREAKING**：现有 `taskUid` 随机占位返回将被真实任务对象替代；部分当前仅为桥接临时设计的方法签名将向官方 SDK 形状收敛。

## Impact
- Affected specs: Index 管理、文档写入、搜索、设置更新、任务模型、TypeScript SDK 兼容层、错误映射
- Affected code: `packages/core/src/engine.rs`、`packages/core/src/index.rs`、`packages/core/src/search.rs`、`packages/core/src/errors.rs`、`packages/core/src/lib.rs`、`packages/core/index.d.ts`、`packages/api/src/index.ts`、`packages/api/__test__/*`、相关 CI 工作流

## ADDED Requirements
### Requirement: 真实任务驱动的索引写入
系统 SHALL 通过真实任务执行路径处理文档写入与设置更新，而不是返回伪造任务结果或仅接受输入不更新可搜索状态。

#### Scenario: 添加文档后可等待任务完成
- **WHEN** 调用 `Index.addDocuments()` 传入合法文档数组
- **THEN** 系统返回包含真实任务标识与状态字段的任务对象
- **AND** 用户可以通过等待接口确认该任务完成
- **AND** 任务完成后，新文档可被后续搜索命中

#### Scenario: 非法文档输入产生稳定错误
- **WHEN** 调用 `Index.addDocuments()` 传入非对象数组、不可序列化值或违反索引约束的数据
- **THEN** 系统返回稳定可判别的桥接错误
- **AND** TypeScript SDK 将其标准化为可由上层代码分支处理的错误对象

### Requirement: 可用的搜索执行路径
系统 SHALL 对外提供可执行的索引搜索能力，并返回与官方 `meilisearch-js` 常用搜索结果结构兼容的字段。

#### Scenario: 搜索已写入文档
- **WHEN** 索引中已有完成任务写入的文档，且用户调用 `Index.search(query, options)`
- **THEN** 系统返回包含 `hits`、总数/估算总数、耗时、查询串等核心字段的结果
- **AND** 返回结果中的文档内容与排序满足当前已启用的基础搜索能力

#### Scenario: 空查询返回基础结果
- **WHEN** 用户调用空查询或等效空白查询
- **THEN** 系统返回与底层搜索引擎约定一致的基础结果集
- **AND** 返回结构仍保持稳定，不因空查询改为特殊占位对象

### Requirement: 基础设置更新
系统 SHALL 支持第一阶段 README 所需的基础索引设置更新，并将变更纳入真实任务执行路径。

#### Scenario: 更新可搜索字段
- **WHEN** 用户调用 `Index.updateSettings()` 传入 `searchableAttributes` 等第一阶段支持的设置字段
- **THEN** 系统返回真实任务对象
- **AND** 等待任务完成后，后续搜索行为体现新的设置

### Requirement: TypeScript SDK 对齐官方常用形状
系统 SHALL 优先对齐官方 `meilisearch-js` 的常用类、方法签名、返回值和错误使用体验，同时保留当前“本地嵌入式引擎”这一非 HTTP 架构的必要差异。

#### Scenario: 官方风格的索引使用体验
- **WHEN** 应用代码通过 `Client` / `Index` 调用索引创建、文档写入、搜索、设置更新与任务等待
- **THEN** 方法命名、参数组织与返回结构尽量接近官方 SDK 的常用调用方式
- **AND** 对于无法完全一致的本地嵌入式差异，代码与类型中有清晰边界而非隐式偏差

### Requirement: SDK 级回归测试
系统 SHALL 提供覆盖第一阶段 Index 核心能力的自动化测试，以防止后续改动退化为占位实现。

#### Scenario: 主路径集成测试通过
- **WHEN** 运行 `packages/api` 的测试套件
- **THEN** 至少覆盖索引创建、添加文档、等待任务、执行搜索、更新设置、错误映射等主路径
- **AND** 失败信息能直接指向对用户可见的行为回归

## MODIFIED Requirements
### Requirement: Index 文档写入返回值
系统 SHALL 不再以随机 `taskUid` 或“已接受文档数”作为唯一结果，而是返回与官方 `meilisearch-js` 常用任务对象兼容的结构，并允许调用方追踪任务最终状态。

### Requirement: Index 搜索行为
系统 SHALL 将当前“始终抛出未实现错误”的 `search` 替换为真实搜索执行逻辑，并返回稳定的搜索结果对象。

### Requirement: SDK 错误映射
系统 SHALL 在 Rust core 与 TypeScript SDK 之间维护稳定错误映射，使调用方能通过明确的错误代码或类别处理常见失败场景，而不是仅依赖字符串消息。

## REMOVED Requirements
### Requirement: 占位式任务结果
**Reason**: 随机生成的 `taskUid` 和固定 `enqueued` 状态不能代表底层是否真实完成写入，也无法支撑后续设置更新与等待能力。
**Migration**: 调用方应改为消费真实任务对象，并在需要读取最新索引状态前等待任务完成。

### Requirement: Search 未实现占位
**Reason**: README 已将 `search` 列为第一阶段可用能力，继续保留占位实现会导致 SDK 对齐目标失效。
**Migration**: 调用方无需额外迁移；原先捕获“未实现”错误的临时代码应切换为处理真实搜索结果或真实搜索错误。
