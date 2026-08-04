# TODO — 长远方向与 deferred 事项

> 这些事项**不是当前 ash-gui-vue 计划的一部分**(当前计划见 `docs/plans/old/039-041-*.md`),而是等完整 ash-gui 跑通后再评估的方向。按领域分组,不承诺时间。

## 真终端专属能力(ash-gui-vue 范围外)

这些是 TUI/CLI ash 有、但一个 webview GUI 天然做不了的——需要**嵌入式终端模拟器**方案才能支持。参考 `ash-core/src/cmd/interactive.rs:10-52` 的交互命令清单。

- [ ] **全终端模拟 / 交互命令**:`vim`、`top`/`htop`、`ssh`、`tmux`、`man`、`psql`、`python` 等需要 PTY + 原始模式的程序,在 GUI 里无法直接运行。
  - **候选方案 A(嵌入终端)**:集成 WebView 内嵌终端(如 `@xterm/xterm` 前端 + 后端 PTY 桥,或 `go-horizontal`/`ttyd` 类方案),为"需要交互的命令"打开一个终端面板。类似 VS Code 的集成终端。
  - **候选方案 B(外部程序)**:检测到交互命令时,用系统终端(Windows Terminal / Alacritty / kitty)启动,命令以 `ash -c "..."` 传递。最简单,但跳出 GUI。
  - **候选方案 C(专用协议)**:对少数高频命令(编辑器)做"编辑器面板"定制(如内置文件树 + 编辑器),而不是通用终端模拟。对齐 ash 的"结构化原生"定位。
- [ ] **raw-mode / 备用屏幕 / 真彩检测**:`ash-tui/src/term/color.rs`、`commands.rs:98-117` — 这些是终端内部机制,GUI 不需要(webview 自有渲染)。
- [ ] **OS 级管道直通**:`ExternalStream::into_raw_stdout`(`external_stream.rs:97-111`)— 把子进程 stdout 直接连到终端 fd,GUI 场景应改用流式事件(见 040 M4)。

## 需要谨慎评估的架构项

- [ ] **`.at` 复刻**:把 ash-gui-vue 的前端组件用 Auto 语言重写(组件命名/导入已对齐 `VueMode::Shadcn` 生成器输出,迁移 = 换生成器输出)。后端 `#[tauri::command]` 可被 `api/targets/tauri.rs` 的 `#[api]` 替代。这是 Plan 039 的既定衔接方向,但工作量未评估。
- [ ] **iced 版去留**:`ash-gui-bin`(iced 原型)保留为参考。当 Vue 版达到功能等价后,评估是否归档。
- [ ] **跨后端一致性**:TUI / iced / Vue 三端的配色与状态语义应统一到单一来源(当前分散在各前端)。可能抽一个共享 token 定义。

## 与外部生态的协同

- [ ] **auto-lang WIP 稳定性**:另一个 agent 在 master 上实时修改 auto-lang(Plan 364/380)。当前 ash-gui-vue 联调依赖 auto-lang 编译稳定。长期看,ash 的构建应能容忍 auto-lang 的中间状态(如 pin 到稳定 commit)。
- [ ] **测试覆盖**:ash-gui-vue 目前无前端测试(无 vitest/playwright)和后端命令测试。跑通完整功能后补。
