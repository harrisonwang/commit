# AGENTS.md

本文件为 Coding Agent 在本仓库工作时提供开发上下文和约束。用户安装、日常使用和产品说明见 `README.md`；更长的设计背景见 `docs/`。

## 常用命令

```bash
cargo build --workspace
cargo fmt --all --check
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace -- --test-threads=1
cargo test -p commit-core --test message
cargo test -p commit-core --test runtime -- --test-threads=1
cargo test -p commit-cli --test cli
```

在有 staged changes 的 Git 仓库中 smoke test CLI：

```bash
COMMIT_HOME="$(mktemp -d)" cargo run -p commit-cli -- prepare
```

CLI 默认依赖外部 `llm` 可执行文件。测试时可通过 `COMMIT_HOME` 或 `~/.commit/config.toml` 覆盖配置。

## 代码结构速览

`commit` 现在是 Cargo workspace：

- `crates/commit-core`：可复用的 Git/context/message/planning/runtime service API，不依赖 clap 或 MCP SDK。
- `crates/commit-cli`：用户安装的 `commit` binary，负责 clap 解析和终端输出。
- `crates/commit-mcp`：MCP tool 边界和 `commit mcp` stdio entrypoint。

主要 core 模块：

- `config.rs`：读取 `~/.commit/config.toml` 或 `$COMMIT_HOME/config.toml`。
- `runtime.rs`：提供 `prepare`、`execute`、`generate`、`run_default_commit` 等 service API。
- `git.rs`：唯一直接 shell out 到 Git 的层，收集 staged diff/stat/name-status/recent subjects，及 `file_content_at(rev, path)`。
- `llm.rs`：构造 prompt，通过 `LlmBackend` 调用外部 `llm` 或测试/外部 message backend。
- `planner.rs`：按粗粒度逻辑类别分组 staged files，并渲染拆分建议。
- `semantic.rs`：基于 tree-sitter 的 AST 语义分析，提取 Rust 文件 top-level 符号变更（Added / Removed / BodyModified / SignatureChanged）。默认 runtime 关闭（`[ast].enabled = false`）。
- `confidence.rs`：把本地 Git 信号和 LLM critique 转成 1-5 分路由置信度。
- `message/`：无外部依赖的 Conventional Commits parser/linter。parser 返回 typed AST，lint 执行项目策略。
- `history.rs`：将 generated/final message pair 写到 `~/.commit/history.jsonl`，并注入后续 prompt。

## 开发约束

- 默认只读取 staged diff，不读取 unstaged diff。
- 不自动 stage 文件。
- 不绕过已有 Git hooks。
- 不把 `commit` 做成 shell alias。
- `commit` 调用外部 `llm` 时只解析 stdout；token、费用和日志应在 stderr。
- commit message 语言规则是刻意设计：Conventional Commit type/scope 前缀用英文，description/body 用简体中文。
- parser/linter 分层很重要：parser 接受规范允许的结构，lint 执行项目策略；不要把 lint policy 下沉到 parser。
- Agent/MCP 集成应使用各 Agent 官方注册方式；Homebrew 不应修改 Agent 配置，MCP server 也不应自注册。

## 测试注意事项

集成测试位于各 crate 的 `tests/` 目录。`commit-core/tests/common/mod.rs` 提供 Git 仓库和 fake `llm` helper。runtime 测试会修改进程环境变量（`PATH`、`COMMIT_HOME`、`GIT_EDITOR`），完整测试需要串行运行：

```bash
cargo test --workspace -- --test-threads=1
```

迭代时常用定向测试：

```bash
cargo test -p commit-core --test message
cargo test -p commit-core --test runtime -- --test-threads=1
cargo test -p commit-cli --test cli
```

提交 parser/runtime/CLI 相关工作前，至少跑完整验证：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace -- --test-threads=1
```

## 文档索引

- `README.md`：用户入口，包含安装、基础用法、核心行为和文档链接。
- `docs/design.md`：架构设计、流水线、关键设计原则和非目标。
- `docs/cli-ux.md`：CLI 用户体验和 flags 边界。
- `docs/agent-integration.md`：Skill / MCP / Direct CLI 三种 Agent 形态、安装方式和产品边界。
- `docs/ast-analysis.md`：基于 tree-sitter 的 AST 语义分析方案与里程碑。
- `docs/conventional-commits-coverage.md`：Conventional Commits parser/linter 覆盖矩阵。
