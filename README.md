# commit

`commit` 是一个独立的 Rust CLI，用来基于 Git staged diff 生成、校验并执行高质量 commit。

它保留两个产品入口：

- 人类终端用户运行裸 `commit`，进入 TTY 预览交互。
- Agent 用户通过 `commit mcp` 调用 MCP tool。默认只暴露单工具 `commit_staged`，把生成、确认和提交收敛到一次 Agent tool call。

`commit` 不是编辑器 prompt，也不是 Agent Skill。它的边界是 commit transaction layer：尊重 staged area、校验 Conventional Commit message、用 `git commit -F` 安全落地，不绕过 Git hooks。

## 安装

CLI 用户推荐通过 Homebrew 安装：

```bash
brew install harrisonwang/homebrew-tap/commit
```

裸 `commit` 默认会调用外部 `llm` 命令。请先确保 `llm` 已安装并完成 provider 配置。

## CLI TTY

```bash
git add <files>
commit
```

TTY 中会展示一条候选 commit message：

```text
[Enter] 提交  [e] 编辑  [r] 换一条  [Ctrl-C] 取消
```

核心行为：

- 默认只读取 staged diff，不读取 unstaged diff。
- 不自动 stage 文件。
- 不绕过已有 Git hooks。
- 使用 `git commit -F <tempfile>` 创建提交。
- commit message 使用 Conventional Commits：type/scope 前缀为英文，description/body 为简体中文。
- TTY 预览路径不会因为 staged 看起来跨多个文件类别而拒绝生成；用户已通过 `git add` 表达提交意图，预览卡片是审稿环节。

非 TTY 环境不会自动提交。Agent 集成请使用 MCP。

## MCP

启动 stdio server：

```bash
commit mcp
```

MCP server 不内置任何模型 API key，也不直接调用 OpenAI、Anthropic、DeepSeek 等 provider。支持 Sampling + Elicitation 的客户端使用 one-call 工具：

| Tool | 用途 |
| --- | --- |
| `commit_staged` | 一次 tool call 内收集 staged 上下文，通过 `sampling/createMessage` 请求客户端模型生成 message，通过 `elicitation/create` 让用户确认或编辑，然后校验并提交 |

Agent 流程：

```text
1. Agent 调用 commit_staged
2. commit server 调 sampling/createMessage 生成 message
3. commit server 调 elicitation/create 让用户确认或编辑
4. 用户接受后，commit server 校验并执行 git commit -F
```

默认不向 Agent 暴露两步 fallback tools，避免客户端在 Sampling / Elicitation 失败时绕回“Agent 自己生成再直接提交”的长链路。需要调试时可显式设置 `COMMIT_MCP_ENABLE_FALLBACK_TOOLS=1`，此时额外暴露 `prepare_commit` 和 `execute_commit`。

Claude Code 注册示例：

```bash
claude mcp add --transport stdio --scope user commit -- commit mcp
```

`commit` 不会自动修改任何 Agent 配置；请使用对应 Agent 的官方 MCP 注册方式。

## 配置

可选配置位于 `~/.commit/config.toml`：

```toml
[llm]
command = "llm"
profile = "deepseek"

[message]
max_subject_chars = 80
direct_commit_threshold = 5
editor_threshold = 3

[validation]
allowed_types = ["feat", "fix", "docs", "test", "ci", "build", "deps", "chore", "refactor", "perf", "style"]
banned_phrases = []

[ast]
enabled = false
max_file_bytes = 200000
```

`[llm]` 只影响裸 `commit` 的 TTY 生成路径。MCP 路径通过客户端 Sampling 使用宿主 Agent 的模型，不读取 `[llm]`。
