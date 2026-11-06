//! wil2atlas（vendored 子集）库入口：只转出 WIL/WIX 与 WZL/WZX 解码所需的四个模块。
//!
//! 上游工具全集另有 `main`（CLI）/ `atlas_packer`（图集打包）/ `indexed`（索引图导出）/
//! `meta`（meta JSON 结构），本子集**刻意不含** —— 下游 `mir2-assets` 只用解码，
//! 公开仓库不该顺带一整套离线图集烘焙工具。
//!
//! 四个模块的内容与上游逐字节一致（见 `../NOTICE` 的 SHA256），未改一行解码逻辑。
//!
//! # 为什么这里有两条 crate 级 lint 指令
//!
//! 决策（2026-11-06）：vendored 模块**不为了迎合新版本 lint 去改源码** ——
//! 一旦改了，`NOTICE` 里那份「逐文件 SHA256 = 上游」的 provenance 保证就断了，
//! 后续上游修正也无法用「整文件替换」的方式同步。风格类 lint 与解码行为无关，
//! 故统一在 crate 根放行，而不是散落到模块里加 `#[allow]`：
//!
//! - `clippy::manual_range_contains`：上游 `wzl_decoder.rs` 里
//!   `w >= MIN && w <= MAX` 的写法（语义等价于 `(MIN..=MAX).contains(&w)`）；
//! - `unsafe_code`：本子集本身无 `unsafe`，这条是**加严**而非放行，
//!   防止日后 vendored 代码被塞入 `unsafe`。
#![allow(clippy::manual_range_contains)]
#![forbid(unsafe_code)]

pub mod image_lib;
pub mod palette_client;
pub mod wil_decoder;
pub mod wzl_decoder;
