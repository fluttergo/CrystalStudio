//! `mir2-stamp` —— CrystalStudio `.X` **印章读写往返保真**库（编辑器「印章体系」地基，审计 P0-10）。
//!
//! 本 crate 做两件事，都在「印章」这一层，都不碰 GUI：
//!
//! 1. **字节层**（[`Stamp`] / [`StampCell`] / [`StampCellData`]）：把 `.X` 解析成结构体，
//!    再原样写回，**逐字节相等**。不做 CLI、不猜字段语义、不做任何「顺手归一化」。
//! 2. **目录层**（[`catalogue`] 模块，2026-11-04 新增）：读 `catalogue/palette.json` 的
//!    9,159 条分类记录 + 扫 `Objects/` 的 9,170 个 `.X`，产出可按分类 / 区域 / 名称查询、
//!    并能把 id 解析成「真实路径 + 包围盒 + 格数」的索引，供 GUI「印章」页签使用。
//!    索引口径与「缺失 / 损坏怎么算」写在该模块文档里（**逐条可复算**）。
//!
//! 真值来源：上游 CrystalStudio 的 `CrystalStudio/CrystalStudio/Document.cs`（**只读参照，未改动**；
//! 对齐基线提交见根 `README.md`，本仓不内置它的本地路径）。
//! 全部行号均指该文件；`ForegroundImages.cs` 的行号单独标注。
//!
//! # 一、文件级字节布局（`CellCodec.SaveStamp`，`Document.cs:137-147`）
//!
//! ```text
//! 偏移  长度  内容                                    真值
//! 0     1     'C' (0x43)                              Document.cs:144
//! 1     1     '#' (0x23)                              Document.cs:144
//! 2     1     版本号 = 2                              Document.cs:144
//! 3     4     格数 count（int32 小端）                 Document.cs:144
//! 7     …     count 个格，首尾相接                     Document.cs:145
//! ```
//!
//! 头固定 **7 字节**（[`HEADER_LEN`]）；文件里**没有**宽高、没有调色板、没有尾注。
//! 实测 9,170 个真实文件：`7 + Σ单格字节` **精确等于**文件长度，**尾部 0 字节冗余**
//! （见 [`trailing_bytes`] 与 `roundtrip_tests` 的「尾随字节必须为 0」用例）。
//!
//! # 二、逐格字节布局（**v2，`map: false` = 印章分支**）
//!
//! `SaveStamp` 先写 `X`/`Y`（`Document.cs:145`），再调 `Write(w, c, map: false)`
//! （`Document.cs:84-98`，其中 `:86` 是四字段共用段、`:94-96` 是印章分支）。
//! 偏移相对**格起点**，整数一律**小端**：
//!
//! | 格内偏移 | 长度 | 字段 | C# 类型 | 真值 |
//! |---------:|-----:|------|---------|------|
//! | 0  | 4 | `X`             | `int`    | `:145` |
//! | 4  | 4 | `Y`             | `int`    | `:145` |
//! | 8  | 2 | `BackLibrary`   | `short`  | `:86` |
//! | 10 | 4 | `BackImage`     | `int`    | `:86` |
//! | 14 | 2 | `MiddleLibrary` | `short`  | `:86` |
//! | 16 | 2 | `MiddleImage`   | `ushort` | `:86` |
//! | 18 | 1 | 前景个数（0/1） | `byte`   | `:94` |
//! | 19 | 2 | 前景 `Library`  | `short`  | `:95` |
//! | 21 | 2 | 前景 `Image`    | `short`  | `:95` |
//! | 23 | 1 | 前景 `Frames`   | `byte`   | `:95` |
//! | 24 | 1 | 前景 `Tick`     | `byte`   | `:95` |
//! | 25 | 1 | `Door`          | `byte`   | `:96` |
//! | 26 | 1 | `DoorOffset`    | `byte`   | `:96` |
//! | 27 | 1 | `MiddleFrames`  | `byte`   | `:96` |
//! | 28 | 1 | `MiddleTick`    | `byte`   | `:96` |
//! | 29 | 2 | `TileAnimation` | `short`  | `:96` |
//! | 31 | 2 | `TileOffset`    | `short`  | `:96` |
//! | 33 | 1 | `TileFrames`    | `byte`   | `:96` |
//! | 34 | 1 | `Light`         | `byte`   | `:96` |
//!
//! ⇒ **无前景 = 29 字节/格**（[`CELL_BASE_LEN`]）；**有前景 = 35 字节/格**（+[`FRONT_LEN`] 6）。
//! 与 `Document.cs:110` 的 `minimumCellBytes = version == 2 ? 29` 自洽。
//!
//! ## 与 README「29 / 35 字节」口径的关系
//!
//! 本仓文档里流传的「每格 29 字节（无前景）/ 35 字节（有前景）」**与实现一致**：
//! 29 = 上述 `0..29`（含 `X`/`Y` 各 4 字节），35 = 29 + 6（前景块）。
//! 真值原文见 `CrystalStudio/README.md:187-201`（字段表）与 `:211-215`
//! （范例 `mir_00000.X`：`7 + 25 × 29 + 8 × 6 = 780` 字节）。
//! 本库实测该文件 = **780 字节、25 格、8 格带前景**，与 README 范例**逐项吻合**。
//!
//! 唯一要补的限定是「**这组数只对 v2 成立**」：v0 是 34 字节/格（无个数字节、前景只有 4 字节、
//! `Frames/Tick` 在 `Door` 之后），v1 是 31 字节/格（无前景）/ 35 字节/格（有前景，前景块 4 字节）。
//! 详见 [`cell`] 模块文档 §三。
//!
//! # 三、`map: true`（地图）分支与印章分支**字节顺序不同**，不可混用
//!
//! `Document.cs:87-93` 的地图分支把前景写成**固定 8 字节槽**（`Library`、`Image`、`Door`、
//! `DoorOffset`、`Frames`、`Tick` 交错），且**没有前景个数字节**；
//! 印章分支（`:94-96`）是「**先写个数，再写 6 字节前景块，`Door/DoorOffset` 排在后面**」。
//! 因此单格地图体是 26 字节、印章体是 21+6n 字节 —— **两套布局互不兼容**。
//!
//! # 四、v0 / v1 只读兼容，且**必然不能字节往返**
//!
//! 读取器兼容 v0/v1/v2（`:108`），但 `SaveStamp`（`:137-147`）**只写 v2**。
//! 重新保存会把格体重新排版 ⇒ v0/v1 文件**只能语义往返，不能字节往返**。
//! 这是 C# 的既定归一化行为，本库照抄，并在 `roundtrip_tests` 里给出实测证据
//! （真实 9,170 个文件**全是 v2**，所以字节往返门 100% 成立）。
//!
//! # 五、位语义：只保存，不解释
//!
//! `BackImage` 高位（`0x60000000` / `0x1FFFFFFF`）与前景 `Image` 的 `0x8000` 位
//! 在编辑器里被当标记位用（`NativeCoastSupport.cs:161`、`RiverBridgeWalkSupport.cs:31`、
//! `StampIdentifier.cs:84` 等）；CrystalStudio 自带说明 `CrystalStudio/README.md:205-207`
//! 也把它们解释为「低 29 位是图号、`0x20000000`/`0x40000000` 是低/高阻挡、前景取 `Image & 0x7FFF`」。
//! **但那是编辑器的约定，不是文件格式的约束** —— 位怎么用完全由业务决定。
//! 本库一个位都不碰、不做任何归一化、不因 `image == 0` 就把前景判成「没有」
//! （`ForegroundImages.Count` 的定义是 `Value is null ? 0 : 1`，见 `ForegroundImages.cs:13`）。
//!
//! # 六、用法
//!
//! ```no_run
//! use mir2_stamp::Stamp;
//!
//! let bytes = std::fs::read("mir/mir_00000.X").unwrap();
//! let stamp = Stamp::parse(&bytes).unwrap();
//! assert_eq!(stamp.to_bytes(), bytes);          // 逐字节往返
//! println!("版本={} 格数={} 包围盒={:?}", stamp.version, stamp.cells.len(), stamp.bounds());
//! ```
//!
//! # 七、实测基线（2026-11-04，本机真实资源）
//!
//! - `<dataRoot>/Objects/**/*.X` 共 **9,170** 个（`mir` 9,159 / `StoneTemplate` 8 /
//!   `InsectTemplate` 2 / `WallEntrances` 1），**全部 v2**；
//! - 合计 **403,008** 格（有前景 187,218 / 无前景 215,790）；
//! - `parse → to_bytes` **9,170 / 9,170 逐字节相等，0 失败、0 跳过**；
//! - 单文件 36 B ~ 42,050 B。
//!
//! # 八、目录索引实测基线（2026-11-04，`catalogue` 模块）
//!
//! - `palette.json` 5,092,886 B / **9,159** 条 `objects[]`，**0 条损坏、0 条跳过**；
//! - 9,159 条 `file` **全部**能在 `Objects/` 下找到（缺失 0）；
//! - `Objects/` 实有 9,170 个 `.X` ⇒ **11 个未登记**（与 `CrystalStudio/README.md:160` 一致）；
//! - 不同 `kind` 19 个（前 5：Terrain 1,695 / Cliff 1,434 / Wall 1,152 / Rock 943 / Debris 759）；
//! - `load()` release **48 ~ 83 ms**（5 次实测，页缓存已命中；冷/热拉不开差距的原因见模块文档），
//!   `search()` 单次 **0.070 ms**（1,000 次最小值）。
//!
//! 逐项口径与复算命令见 [`catalogue`] 模块文档 §五，以及 `examples/catalog_probe.rs`。
//!
//! # 九、分类中文标签（[`kind_labels`] 模块，2026-11-04 新增）
//!
//! [`kind_labels::KIND_LABELS_ZH`] 是「`palette.json` 的 `type` → 中文」的**唯一**对照表：
//! **20** 条 = 19 个英文 kind + 1 个空串键（对应 JSON `"type": null` 的 9 条记录，标签「未分类」）。
//! ⇒ 与 §八 的「不同 `kind` 数 19」**不矛盾**：那边只数非 null 的 kind（`null` 不进分类索引），
//! 本表多出来的第 20 行专给 `null` 用。
//!
//! 这张表**只服务 UI 显示**，不参与 `.X` 解析、不参与 `palette.json` 读取、不改任何字段：
//! - 已知 ⇒ 中文（[`kind_labels::kind_label_zh`] 返回 `Some`）；
//! - 未知 ⇒ `None`，或用 [`kind_labels::kind_label_zh_or`] **原样返回英文**（不编、不空白）；
//! - **`subtype`（实测 447 个取值）一律保留英文**，本 crate 不给它任何中文映射（无权威译名，
//!   逐条编就是编数据）。
//!
//! 上游将来新增分类 ⇒ [`kind_labels`] 的真实数据用例**当场红**并列出「漏掉的 kind × 条数」，
//! 不会等到 GUI 上显示成英文才发现。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod catalogue;
pub mod kind_labels;
mod cell;
mod error;
mod stamp;

pub use catalogue::{CatalogError, CatalogStats, ResolvedStamp, StampCatalog, StampEntry};
pub use kind_labels::{kind_label_zh, kind_label_zh_or, KIND_LABELS_ZH};
pub use cell::{
    cell_byte_len, minimum_cell_bytes, Front, StampCellData, CELL_BASE_LEN, FRONT_LEN, HEADER_LEN,
    MAGIC, MAX_CELLS, WRITE_VERSION,
};
pub use error::StampError;
pub use stamp::{trailing_bytes, Stamp, StampCell};

#[cfg(test)]
mod catalogue_tests;
#[cfg(test)]
mod roundtrip_tests;
