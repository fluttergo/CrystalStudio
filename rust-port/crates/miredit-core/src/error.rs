//! 内核错误类型：手写 `Display` + `Error`，不引 thiserror（依赖只许 `std` + `mir2-map` + `mir2-format`）。
//!
//! # 什么进 [`CoreError`]，什么只算"拒绝"
//!
//! | 情形 | 返回 | 依据 |
//! |---|---|---|
//! | 读文件失败 | `Err(CoreError::Io)` | 结构层面无法继续，没有"部分成功"这种状态 |
//! | `.map` 九种格式都判不出来 / 尺寸非法 | `Err(CoreError::Map)` | 判据在 `mir2-format` 里，这里只包装 |
//! | 保存前出字节失败（视图与字节层形状不符 / writer 拒值） | `Err(CoreError::Map)`（payload 是 [`FormatError::Inconsistent`]） | 绝不静默回退到原字节（那等于假保存） |
//! | 命令里的层名不认识 | `Err(CoreError::BadLayer)` | 不能猜成某一层（猜错就是往别的字段写字节） |
//! | 原子保存任一阶段失败 | `Err(CoreError::Save { stage, … })` | 阶段必须可区分，否则"保存失败"无法定位 |
//! | 族不匹配 / 阻挡层收到图库笔刷 / 填充层不支持 / 越界 / Type5 地表落笔 | `Ok(Outcome { changed: 0, note: Some(…) })` | 任务书 §七「拒绝必须返回明确 outcome，不许静默不改」——这些是**用户输入层面的拒绝**，不是程序故障；调用方要能同时拿到 `dirty` / `can_undo` 等状态 |
//!
//! 换句话说：`Err` 表示"这次调用没有完成"，`Outcome.changed == 0 + note` 表示
//! "调用完成了，但按语义一格都不该改，原因在这里"。
//!
//! # 为什么「出字节失败」复用 [`CoreError::Map`] 而**不新开变体**
//!
//! `miredit-web/src/error.rs:117-128` 对 [`CoreError`] 做了**穷尽 match**（四个变体各一条），
//! 而本单**不允许改 `miredit-web/**`**（任务书 §一）。新开第五个变体 ⇒ 整个 workspace 编译不过。
//! ⇒ 把"出字节失败"塞进 payload 类型本来就对的 [`CoreError::Map`]
//! （它的 payload 是 [`FormatError`]，而 writer 拒绝时返回的正是 [`FormatError::Inconsistent`]），
//! 同时把该变体的语义与 `Display` 放宽到**读/写两个方向**，不留"说是解析、其实是写"的谎话。

use std::fmt;
use std::path::PathBuf;

use mir2_format::FormatError;

/// 原子保存失败时**停在哪一步**（阶段名必须可区分）。
///
/// 对应 `miredit-gui/src/save.rs:186-260` 的五条失败分支：写临时文件 / `sync_all` /
/// 读回核对 / `rename`。GUI 用五条不同的文案模板表达同一件事，本 crate 用一个枚举
/// 表达，`Display` 里再折成一句中文 —— 阶段信息不丢。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStage {
    /// ① 写临时文件（`File::create` 或 `write_all` 失败）。
    Temp,
    /// ①' `sync_all()` 刷盘失败（此时**不进** rename 阶段，宁可原文件一字节不动）。
    Sync,
    /// ② 读回逐字节核对失败（短写 / 缓写）。
    Verify,
    /// ③ `rename` 原子替换失败。
    Rename,
}

impl SaveStage {
    /// 阶段的中文短名（进错误信息与汇报，便于 grep）。
    pub const fn label(self) -> &'static str {
        match self {
            SaveStage::Temp => "写临时文件",
            SaveStage::Sync => "刷盘",
            SaveStage::Verify => "读回核对",
            SaveStage::Rename => "原子替换",
        }
    }
}

/// 编辑内核的失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// 读/写文件失败（`detail` 是系统报错原文，不加工）。
    Io {
        /// 出问题的路径。
        path: PathBuf,
        /// 系统报错原文（`std::io::Error` 的 `to_string()`）。
        detail: String,
    },
    /// `.map` 的**格式层**失败（**读或写**两个方向）。
    ///
    /// 读方向：九种格式的判别式全部不成立 / 尺寸非法 / 超格数 / 长度不足。
    /// 写方向：`Document::try_to_bytes` 的「视图与字节层形状不符」（包装成
    /// [`FormatError::Inconsistent`]）或 writer 拒绝某个值。
    ///
    /// 解析规则本身在 `mir2-format` 里（九种格式的注册表），这里只做包装，**不重写判据**。
    /// ⚠ 为什么写方向也复用本变体而不新开一个：见模块文档最后一节（`miredit-web` 的穷尽 match）。
    Map {
        /// 被解析的路径（诊断用）。
        path: PathBuf,
        /// `mir2-format` 给出的具体原因（九种格式的判别/长度/尺寸/写前校验全部在里面）。
        err: FormatError,
    },
    /// 命令里的层名不认识。
    ///
    /// 合法取值只有 [`crate::EditLayer::key`] 的五个 ASCII 键
    /// （`ground` / `middle` / `front` / `low_collision` / `high_collision`）。
    /// 绝不"猜一个最像的"——猜错就是把图号写进别的层的字段。
    BadLayer {
        /// 调用方传来的原始字符串（原样回显，便于定位是谁传错了）。
        key: String,
    },
    /// 原子保存失败（原文件保持原样）。
    Save {
        /// 失败发生在哪一步。
        stage: SaveStage,
        /// 目标路径。
        path: PathBuf,
        /// 涉及的临时文件路径（`Temp` / `Sync` / `Verify` / `Rename` 四步都有）。
        tmp: PathBuf,
        /// 系统报错原文或长度对比（不加工）。
        detail: String,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Io { path, detail } => {
                write!(f, "读写文件失败：{} ⇒ {detail}", path.display())
            }
            CoreError::Map { path, err } => {
                write!(f, "地图格式层失败（读/写）：{} ⇒ {err}", path.display())
            }
            CoreError::BadLayer { key } => write!(
                f,
                "层名不合法：{key:?}（只接受 ground / middle / front / low_collision / high_collision）"
            ),
            CoreError::Save {
                stage,
                path,
                tmp,
                detail,
            } => write!(
                f,
                "保存失败于「{}」阶段：目标={} 临时={} ⇒ {detail}（原文件未改动）",
                stage.label(),
                path.display(),
                tmp.display()
            ),
        }
    }
}

impl std::error::Error for CoreError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 阶段名可区分() {
        let all = [
            SaveStage::Temp,
            SaveStage::Sync,
            SaveStage::Verify,
            SaveStage::Rename,
        ];
        let mut names: Vec<&str> = all.iter().map(|s| s.label()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 4, "四个阶段的短名必须互不相同");
    }

    /// 保存错误的信息必须同时带**阶段 / 目标 / 临时文件 / 原文**四要素
    /// （`miredit-gui/src/save.rs:419-459` 的用例就是这么钉的：缺一项就无法定位）。
    #[test]
    fn 保存错误四要素齐全() {
        let e = CoreError::Save {
            stage: SaveStage::Rename,
            path: PathBuf::from(r"I:\x\c.map"),
            tmp: PathBuf::from(r"I:\x\c.map.miredit-tmp"),
            detail: "拒绝访问。 (os error 5)".to_string(),
        };
        let msg = e.to_string();
        for 片段 in ["原子替换", "c.map", "miredit-tmp", "os error 5", "原文件未改动"] {
            assert!(msg.contains(片段), "信息缺「{片段}」：{msg}");
        }
    }

    #[test]
    fn 层名错误原样回显() {
        let e = CoreError::BadLayer {
            key: "groundd".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("groundd"), "{msg}");
        assert!(msg.contains("low_collision"), "要列出合法取值：{msg}");
    }

    #[test]
    fn 可当标准错误装箱() {
        let e: Box<dyn std::error::Error> = Box::new(CoreError::BadLayer {
            key: "x".to_string(),
        });
        assert!(e.to_string().contains('x'));
    }
}
