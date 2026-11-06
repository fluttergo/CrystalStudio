//! 验收门 2/3/4/5/6 的**逐字节**取证 —— 全部走 GUI 的真实入口。
//!
//! # 一、这些用例为什么在 `miredit-gui` 里，而不在 core 里
//!
//! core 的 `tests/stamp_place.rs` / `tests/selection_clipboard.rs` 验的是**内核语义**；
//! 本模块（`acceptance_tests/`）验的是**接线**：同一批操作经过
//! [`crate::edit::EditSession`]（宿主 ↔ 内核的地图交换层）+
//! [`crate::stamp_place`]（读盘 → 注册 → 落位）+
//! [`crate::selection_ops`]（快捷键命令）+
//! [`crate::save::save_session`]（原子写回）之后，**磁盘上的字节**是否仍然只有该变的那几格变。
//!
//! 交换层（`with_map` 用 `std::mem::swap` 换地图、`sync_document_format` 重建文档）
//! 是宿主独有的风险点：它可能让"内核算的计划"和"宿主写的地图"变成两份。
//! 只有逐字节比对能证明它们是一份。
//!
//! # 二、本机没有的东西一律**跳过并打印**，绝不假绿
//!
//! | 依赖 | 缺失时 |
//! |---|---|
//! | `<资产根>/GodotClient/maps/0.map`（700×700 / 12 B 每格；资产根 = `MIREDIT_ASSET_ROOT`） | 打印 `[跳过]` 并返回（不算失败） |
//! | `<取证根>/Objects/mir/mir_00000.X`（25 格；取证根 = `MIREDIT_UPSTREAM_ROOT`） | 同上 |
//!
//! 换机器时这两条不会变红，但**本机实跑时一定打印出真实数字**（汇报里的证据就是它）。
//!
//! # 三、门 6 是**注入验证**（如实说明）
//!
//! ⚠ **2026-11-05 起这条说明只对 `selection.rs` 的门 6 成立**：GUI **已经能打开 Type5 文件**
//! （装载走 `miredit_core::Document::load` → `mir2_format::read_any_tolerant`，九种格式全支持）。
//! Type5 的**真装载路径**验收在 `formats.rs` 的 `门4_type5真装载路径_格式名与地表能力_前端拦截`。
//!
//! `selection.rs` 的门 6 验的是**另一件事**：内核 `plan_stamp` 的 Type5 闸门
//! （"贴带地表的内容"要被拒）。它用 `inject_document` 把档位塞进会话，
//! 好处是**不必真的落一个 Type5 文件**就能验内核语义，且与宿主装载路径解耦。
//! 两门不重复：一个验"内核语义"，一个验"宿主装载 + 前端拦截"。

//! # 四、文件划分（单文件 ≤600 行）
//!
//! | 文件 | 内容 |
//! |---|---|
//! | `mod.rs`（本文件） | 模块文档 + 子模块声明 |
//! | `common.rs` | 本机样本路径 / 临时目录 / 逐字节差异 / 偏移转格 / 找地表 3×3 / 合成样本 |
//! | `stamp.rs` | 门 2（印章放置逐字节）+ 门 3（撤销逐字节）|
//! | `selection.rs` | 门 4（复制粘贴 3×3）+ 门 5（剪切 3×3）+ 门 6（Type5 拒绝，注入验证）|
//! | `formats.rs` | **九格式验收**：全仓 1,158 张开图 / 未改即存 0 差异 / Type5 真装载路径 / 67 张带尾块图的黄条 |

mod common;
mod formats;
mod selection;
mod stamp;
