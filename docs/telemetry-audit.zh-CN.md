# BitFun 项目埋点梳理

## 结论

当前项目里，真正可归类为“埋点”的实现主要有两类：

1. OpenTelemetry 遥测
   仅在 Desktop 端初始化，默认开启采集能力；是否真正发往远端，取决于是否配置 OTLP exporter。
2. 本地 usage 统计
   主要是 token usage 落盘，以及基于会话持久化数据生成 Insights 报告。

另外，项目中大量 `EventEmitter` / `EventBus` / `AgenticEvent` 属于内部事件分发机制，不等同于外部分析埋点。

本次审计未发现前端接入第三方分析 SDK，例如 GA、Mixpanel、PostHog、Amplitude、Segment、Sentry 一类的上报代码。

## 审计范围

本次主要检查了以下模块：

- `src/apps/desktop/src/lib.rs`
- `src/crates/core/src/infrastructure/telemetry/`
- `src/crates/core/src/infrastructure/ai/client.rs`
- `src/crates/core/src/service/token_usage/`
- `src/crates/core/src/agentic/insights/`
- `src/crates/core/src/agentic/events/router.rs`
- `src/web-ui/src/infrastructure/event-bus/EventBus.ts`

## 一、真正的 OpenTelemetry 埋点

### 1. 初始化入口

Desktop 启动时会初始化全局 telemetry，且当前是硬编码 `enabled: true`：

- `src/apps/desktop/src/lib.rs:91`
- `src/apps/desktop/src/lib.rs:97`

初始化成功后会先打一个 `app_launch_started` 事件，表示“启动流程开始”。
在 Desktop `setup` 完成后，会再打一个 `app_launch_succeeded` 事件，表示“应用启动成功完成”，并附带 `startup_duration_ms`：

- `src/apps/desktop/src/lib.rs:100`
- `src/apps/desktop/src/lib.rs:315`

如果启动阶段失败，会打 `app_launch_failed`。如果应用已经成功启动、随后在运行阶段报错，则会打 `app_runtime_failed`。

主窗口关闭时，会先打 `app_exit_requested`，再打 `app_closed`，随后执行 telemetry shutdown。发生 panic 时还会打 `app_crashed`：

- `src/apps/desktop/src/lib.rs:316`
- `src/apps/desktop/src/lib.rs:319`
- `src/apps/desktop/src/lib.rs:321`
- `src/apps/desktop/src/lib.rs:917`
- `src/apps/desktop/src/lib.rs:929`
- `src/apps/desktop/src/lib.rs:1032`

### 2. 公共上下文字段

所有 telemetry event/span 都会自动补齐以下公共属性：

- `timestamp`
- `uid`
- `process_session_id`
- `ide_version`
- `os`
- `os_version`
- `arch`
- `app_name`
- `app_kind`
- `event_name`

代码位置：

- `src/crates/core/src/infrastructure/telemetry/mod.rs:154`

其中 `uid` 会持久化到本地：

- 路径：`user_data_dir()/telemetry/uid`
- 代码：`src/crates/core/src/infrastructure/telemetry/mod.rs:491`
- 路径生成：`src/crates/core/src/infrastructure/telemetry/mod.rs:521`

这意味着即使不上传业务内容，当前实现仍然会为同一设备/用户维持一个稳定标识。

另外，当前还会补一个进程级启动实例 ID：

- 字段名：`process_session_id`
- 语义：一次应用进程生命周期内固定，进程重启后重新生成

这个字段和业务 `session_id` 不同，前者表示“本次应用启动实例”，后者表示“业务会话 / 对话会话”。

### 3. 当前已接入的 telemetry 事件

`TelemetryEventSubscriber` 注册在 agentic 内部事件路由上：

- 注册位置：`src/apps/desktop/src/lib.rs:810`
- 内部订阅模型：`src/crates/core/src/agentic/events/router.rs:36`

当前代码里实际会发出的 telemetry event 如下。

| 事件名 | 触发来源 | 主要字段 |
| --- | --- | --- |
| `app_launch_started` | Desktop 启动流程开始 | 仅公共上下文 |
| `app_launch_succeeded` | Desktop 启动完成 | `startup_duration_ms`, `success=true` |
| `app_launch_failed` | Desktop 启动失败 | `stage`, `success=false`, `error` |
| `app_runtime_failed` | Desktop 运行期失败 | `stage`, `success=false`, `error` |
| `app_exit_requested` | 主窗口收到关闭请求 | `reason`, `uptime_ms` |
| `app_closed` | Desktop 关闭完成 | `reason`, `uptime_ms` |
| `app_crashed` | 进程 panic | `fatal=true`, `panic_location`, `panic_message`, `runtime_log_upload_*` |
| `chat_request_started` | `DialogTurnStarted` | `session_id`, `turn_id`, `turn_index` |
| `chat_request_completed` | `DialogTurnCompleted` | `session_id`, `turn_id`, `total_rounds`, `total_tools`, `duration_ms`, `success=true` |
| `chat_request_cancelled` | `DialogTurnCancelled` | `session_id`, `turn_id`, `cancelled=true` |
| `chat_request_failed` | `DialogTurnFailed` | `session_id`, `turn_id`, `success=false`, `error` |
| `token_usage_updated` | `TokenUsageUpdated` | `session_id`, `turn_id`, `model_id`, `input_tokens`, `output_tokens`, `total_tokens`, `max_context_tokens`, `is_subagent` |
| `context_compression_started` | `ContextCompressionStarted` | `session_id`, `turn_id`, `compression_id`, `trigger`, `tokens_before`, `context_window`, `threshold` |
| `context_compression_completed` | `ContextCompressionCompleted` | `session_id`, `turn_id`, `compression_id`, `compression_count`, `tokens_before`, `tokens_after`, `compression_ratio`, `duration_ms`, `has_summary`, `success=true` |
| `context_compression_failed` | `ContextCompressionFailed` | `session_id`, `turn_id`, `compression_id`, `success=false`, `error` |
| `model_round_started` | `ModelRoundStarted` | `session_id`, `turn_id`, `round_id`, `round_index` |
| `model_round_completed` | `ModelRoundCompleted` | `session_id`, `turn_id`, `round_id`, `has_tool_calls`, `success=true` |
| `model_round_cancelled` | `ModelRoundCancelled` | `session_id`, `turn_id`, `round_id`, `round_index`, `cancel_reason`, `cancelled=true` |
| `model_round_failed` | `ModelRoundFailed` | `session_id`, `turn_id`, `round_id`, `round_index`, `success=false`, `error` |
| `tool_request_started` | `ToolEvent::Started` | `session_id`, `turn_id`, `tool_id`, `tool_name` |
| `tool_request_completed` | `ToolEvent::Completed` | `session_id`, `turn_id`, `tool_id`, `tool_name`, `duration_ms`, `success=true` |
| `tool_request_failed` | `ToolEvent::Failed` | `session_id`, `turn_id`, `tool_id`, `tool_name`, `success=false`, `error` |
| `tool_request_cancelled` | `ToolEvent::Cancelled` | `session_id`, `turn_id`, `tool_id`, `tool_name`, `cancel_reason`, `cancelled=true` |

其中：

- App 生命周期事件来自：`src/apps/desktop/src/lib.rs`
- Chat / image analysis / token / compression / round / tool 事件来自：`src/crates/core/src/infrastructure/telemetry/mod.rs`

补充说明：

- `ImageAnalysisStarted` / `ImageAnalysisCompleted` 事件类型、transport 适配和前端消费代码目前都还在。
- 但本次复查没有在后端执行链路中找到实际的生产端 `emit/enqueue` 位置。
- 所以它们更准确的状态是“已预留、可被 telemetry 映射”，而不是“当前代码里稳定实际发出”的 event。

### 4. 模型请求 span

除了事件型埋点，AI 请求链路还会创建一个 `model_request` span：

- 创建位置：`src/crates/core/src/infrastructure/ai/client.rs:1336`

初始字段包括：

- `provider`
- `model`
- `api_format`
- `stream`
- `message_count`
- `tool_count`

代码位置：

- `src/crates/core/src/infrastructure/ai/client.rs:1338`

在流式返回过程中，还会继续追加：

- `finish_reason`
- `total_tokens`
- `input_tokens`
- `output_tokens`

代码位置：

- `src/crates/core/src/infrastructure/ai/client.rs:70`

在请求完成或失败时，还会补充：

- `retry_count`
- `status_code`
- `error_type`
- `duration_ms`
- `success`
- `cancelled`
- `cancel_reason`
- `error`

代码位置：

- `src/crates/core/src/infrastructure/ai/client.rs:111`
- `src/crates/core/src/infrastructure/telemetry/mod.rs:191`
- `src/crates/core/src/infrastructure/telemetry/mod.rs:233`

另外，请求上下文还会自动补齐：

- `session_id`
- `turn_id`
- `round_id`
- `is_subagent`

这部分通过 `with_telemetry_request_context` 注入，确保 `model_request` span 能关联到具体对话轮次。

### 5. 远端导出条件

当前 telemetry 虽然默认初始化，但现在已经带了默认 OTLP endpoint：`http://7.183.57.199:4317`。
如果运行时或构建期提供了自定义 endpoint，则仍然按自定义值覆盖。

运行时环境变量支持：

- `BITFUN_OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`
- `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`
- `BITFUN_OTEL_EXPORTER_OTLP_ENDPOINT`
- `OTEL_EXPORTER_OTLP_ENDPOINT`
- `BITFUN_OTEL_EXPORTER_OTLP_PROTOCOL`
- `OTEL_EXPORTER_OTLP_PROTOCOL`

代码位置：

- `src/crates/core/src/infrastructure/telemetry/exporter.rs:74`
- `src/crates/core/src/infrastructure/telemetry/exporter.rs:77`
- `src/crates/core/src/infrastructure/telemetry/exporter.rs:119`

支持协议：

- `grpc`
- `http/protobuf`

代码位置：

- `src/crates/core/src/infrastructure/telemetry/exporter.rs:46`
- `src/crates/core/src/infrastructure/telemetry/exporter.rs:124`

默认情况下会发送到：

- `http://7.183.57.199:4317`

另外也支持在构建期写死默认 OTLP 配置：

- `BITFUN_BUILD_OTLP_ENDPOINT`
- `BITFUN_BUILD_OTLP_PROTOCOL`

代码位置：

- `src/crates/core/build.rs:11`

## 二、本地统计数据

### 1. Token usage 记录

`TokenUsageSubscriber` 会监听 `TokenUsageUpdated` 事件，并写入本地 token usage 存储：

- 订阅器：`src/crates/core/src/service/token_usage/subscriber.rs:25`
- 注册位置：`src/apps/desktop/src/lib.rs:805`

单条记录字段：

- `model_id`
- `session_id`
- `turn_id`
- `timestamp`
- `input_tokens`
- `output_tokens`
- `cached_tokens`
- `total_tokens`
- `is_subagent`

代码位置：

- `src/crates/core/src/service/token_usage/types.rs:7`

存储路径：

- 基础目录：`user_data_dir()/token_usage`
- 聚合文件：`model_stats.json`
- 明细目录：`records/YYYY-MM-DD.json`

代码位置：

- `src/crates/core/src/service/token_usage/service.rs:18`
- `src/crates/core/src/service/token_usage/service.rs:69`
- `src/crates/core/src/service/token_usage/service.rs:74`
- `src/crates/core/src/service/token_usage/service.rs:79`

写入逻辑：

- `src/crates/core/src/service/token_usage/service.rs:141`

这一块是本地统计，不属于 OTEL exporter 链路。

### 2. Insights 报告

Insights 不是单独新增的远端埋点系统，而是基于已有的会话持久化数据做离线分析。

采集来源：

- 遍历 workspace 下的 session 数据
- 加载 session、turn、message
- 构建 transcript 和基础统计

代码位置：

- `src/crates/core/src/agentic/insights/collector.rs:24`
- `src/crates/core/src/agentic/insights/collector.rs:36`
- `src/crates/core/src/agentic/insights/collector.rs:54`
- `src/crates/core/src/agentic/insights/collector.rs:65`
- `src/crates/core/src/agentic/insights/collector.rs:70`

输出路径：

- `user_data_dir()/usage-data/insights-*.json`
- `user_data_dir()/usage-data/insights-*.html`

代码位置：

- `src/crates/core/src/agentic/insights/service.rs:1247`
- `src/crates/core/src/agentic/insights/service.rs:1249`
- `src/crates/core/src/agentic/insights/service.rs:1257`
- `src/crates/core/src/agentic/insights/service.rs:1264`

需要注意的是，Insights 处理的是会话内容与行为统计，因此数据敏感度高于 OTEL 事件本身；但从代码上看，这部分当前是本地读写，不走 OTLP 上报。

## 三、不应误判为“埋点”的部分

### 1. Core EventRouter

`EventRouter` 是内部订阅分发机制，不是外部分析上报通道。

代码已经明确写明是 internal subscribers：

- `src/crates/core/src/agentic/events/router.rs:13`
- `src/crates/core/src/agentic/events/router.rs:38`

它的用途是把 `AgenticEvent` 分发给内部订阅者，例如：

- telemetry
- token usage
- cron

### 2. Frontend EventBus

前端 `EventBus` 也是模块间 pub/sub，不是远端埋点 SDK。

代码位置：

- `src/web-ui/src/infrastructure/event-bus/EventBus.ts:2`
- `src/web-ui/src/infrastructure/event-bus/EventBus.ts:34`
- `src/web-ui/src/infrastructure/event-bus/EventBus.ts:133`

它会记录内存中的事件历史 `eventHistory`，但没有发现将这些事件直接上报外部服务的实现。

## 四、当前埋点覆盖面总结

当前真正已经落地的埋点覆盖面如下：

- 应用生命周期：有
- 对话请求生命周期：有
- 上下文压缩生命周期：有
- 模型 round 生命周期：有
- 工具调用生命周期：有
- 模型请求性能/结果：有
- token 统计：有，但本地落盘
- insights 分析：有，但基于本地会话数据
- 前端页面浏览/点击行为：未发现
- Web/Mobile/Installer 第三方分析 SDK：未发现

## 五、当前风险与问题

### 1. Telemetry 默认开启，但没有看到用户级开关

Desktop 初始化时直接传入 `enabled: true`，这意味着功能默认打开。

问题不在于一定会远端发送，而在于：

- 采集能力默认存在
- 一旦环境或构建里配置了 OTLP endpoint，就会开始导出
- 目前没有发现明确的用户设置项或 consent 流程

### 2. 使用稳定 `uid`

`uid` 会持久化到本地并跨启动复用。对于分析系统来说这很实用，但从隐私和合规角度，它已经是稳定标识。

### 3. 错误信息和业务标识可能出端

当前 telemetry 不上传 prompt / response 正文，这是好事；但会上传：

- `session_id`
- `turn_id`
- `tool_id`
- `tool_name`
- `model`
- `provider`
- `error`

其中 `error` 是原始错误字符串，可能带出接口细节、路径、供应商错误文本，建议再做一次脱敏审视。

### 4. 本地统计和远端遥测边界未文档化

现在代码里边界其实比较清楚，但仓库里没有正式文档，外部很容易把 `AgenticEvent`、Insights、token usage、frontend event bus 全都混叫成“埋点”。

## 六、建议

建议按优先级做下面几件事：

1. 增加正式 telemetry 配置项
   包括总开关、endpoint、protocol、是否允许稳定 `uid`。
2. 增加用户可见说明
   明确区分“远端遥测”和“本地统计/洞察”。
3. 对 `error`、`session_id`、`turn_id` 做脱敏策略评审
   至少确认是否都必须出端。
4. 把前端/内部事件与遥测文档分开
   避免维护和合规讨论时混淆。
5. 给 telemetry 加测试或快照
   至少验证 event name 和字段集合，避免后续无意扩大采集范围。

## 七、最简判断口径

如果只想快速判断“哪些东西算埋点”，可以按下面这条线理解：

- `src/crates/core/src/infrastructure/telemetry/` 里的东西，算真正的遥测埋点。
- `src/crates/core/src/service/token_usage/` 和 `src/crates/core/src/agentic/insights/`，算本地统计数据。
- `EventRouter` / `EventBus` / `emit` 这类，大多数只是内部事件通信，不直接等于外部埋点。
