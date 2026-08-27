//! ash — AutoShell CLI:线性输出 + 尾部动态(Plan 071)
//!
//! lib 目标承载终端前端模块(`frontend/`,原 ash-tui crate 融合迁入),
//! 供 bin(`main.rs`)与集成测试共用。`auto-shell` 保持零终端依赖 —— 037
//! M2.2 的纯逻辑边界不变,消失的只是"两个终端前端"的 crate 区分。

pub mod frontend;

// 根级再导出:迁入模块内部的 `crate::renderer`/`crate::prompt`/… 路径
// 无需改写(Plan 071 Phase 2 纯机械搬迁原则)。
pub use frontend::*;
