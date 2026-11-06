//! 素材层错误类型（手写 `Display` + `std::error::Error`，不引 thiserror）。

use std::fmt;
use std::io;
use std::path::PathBuf;

/// 图库素材层的失败原因。
///
/// 只覆盖「结构层面无法继续」的情况：扫描根不存在、目录读不了、一个库都没登记、单库打开失败，
/// 以及原版 `.lib` 的文件头非法 / 单帧数据损坏（[`AssetError::LibHeaderInvalid`] /
/// [`AssetError::FrameDecodeFailed`]）。
/// **帧级「不可得」（库不存在 / 帧号越界 / 空帧）不走这里**，
/// 按 [`crate::SpriteSource::frame`] 的契约返回 `None`。
#[derive(Debug)]
pub enum AssetError {
    /// 扫描根**未设置**：对应的环境变量没给（或给了空串 / 指向的不是目录）。
    ///
    /// 单独一支而不复用 [`AssetError::RootNotDir`]：公开仓不内置任何开发机路径，
    /// 「没告诉我在哪」与「告诉我了但那里不是目录」是两件事，界面文案必须能分开说。
    RootUnset {
        /// 环境变量名（例如 `MIREDIT_CLIENT_ROOT`）。
        var: &'static str,
    },
    /// 扫描根不存在，或存在但不是目录。
    RootNotDir {
        /// 传入的扫描根路径。
        path: PathBuf,
    },
    /// 文件系统访问失败（读目录 / 打开库 / 读文件头）。
    ///
    /// 注意分层：**打开期**的 IO 失败（目录读不了、库文件打不开、读文件头/偏移表失败）
    /// 走这里；而 `.lib` **按帧 `seek` 读数据**失败发生在单帧层面，
    /// 折成 [`AssetError::FrameDecodeFailed`]（那是「这一帧读不出来」，
    /// 不是「整个库不可用」）。
    Io {
        /// 出错的路径。
        path: PathBuf,
        /// 底层 IO 错误。
        source: io::Error,
    },
    /// 扫描完成但一个可登记的库都没有。
    NoLibraries {
        /// 扫描根路径。
        root: PathBuf,
    },
    /// 单个库文件打开或解析失败。
    ///
    /// `wil2atlas` 的解码器返回 `anyhow::Result`；本 crate 的依赖白名单不允许引入
    /// `anyhow`，故在此把错误链收成一段字符串（保留完整 `{:#}` 上下文）。
    OpenLibFailed {
        /// 库数据文件路径。
        path: PathBuf,
        /// 解码器给出的原因（已含 `anyhow` 上下文链）。
        reason: String,
    },
    /// 原版 `.lib` 的文件头 / 版本 / 偏移表解析失败（`Assets.cs:22-67@901dbe2f` 的构造期校验）。
    ///
    /// 覆盖：版本号不认识、Sanxiu 加密标志非 0、`headerLength` 越界、图片数越界、
    /// Sanxiu 某图的偏移/长度越界、v2/v3 偏移表越界或截断。
    LibHeaderInvalid {
        /// `.lib` 文件路径。
        path: PathBuf,
        /// 具体原因（含版本号、字段值与合法区间）。
        reason: String,
    },
    /// 原版 `.lib` 的单帧解码失败：帧头截断 / `length` 越界 / 尺寸超 8192 /
    /// gzip 损坏 / 解压字节数与 `w*h*4` 不符 / 内嵌图片解不开 / `fileType` 未知。
    ///
    /// 注意区分：**帧号越界**、`w <= 0`、`h <= 0`、Sanxiu `length == 0` 这些在原版里
    /// 是「返回 null」而不是抛异常，本 crate 同样返回 `None`
    /// （见 [`crate::CrystalLibrary::decode_checked`] 的 `Ok(None)`）。
    FrameDecodeFailed {
        /// `.lib` 文件路径。
        path: PathBuf,
        /// 帧号（0-based）。
        index: u32,
        /// 具体原因。
        reason: String,
    },
}

impl AssetError {
    /// 构造「IO 失败」错误（把路径与底层错误一起带上）。
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> AssetError {
        AssetError::Io {
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::RootUnset { var } => write!(
                f,
                "未设置环境变量 {var}（图库扫描根）—— 公开仓不内置任何缺省路径"
            ),
            AssetError::RootNotDir { path } => {
                write!(f, "扫描根不存在或不是目录：{}", path.display())
            }
            AssetError::Io { path, source } => {
                write!(f, "文件访问失败：{}（{source}）", path.display())
            }
            AssetError::NoLibraries { root } => write!(
                f,
                "在 {} 下没有登记到任何图库（要求同名数据+索引成对：.wzl+.wzx 或 .wil+.wix）",
                root.display()
            ),
            AssetError::OpenLibFailed { path, reason } => {
                write!(f, "打开图库失败：{}（{reason}）", path.display())
            }
            AssetError::LibHeaderInvalid { path, reason } => {
                write!(f, "`.lib` 文件头非法：{}（{reason}）", path.display())
            }
            AssetError::FrameDecodeFailed { path, index, reason } => write!(
                f,
                "`.lib` 第 {index} 帧解码失败：{}（{reason}）",
                path.display()
            ),
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssetError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 错误信息含路径与原因() {
        let e = AssetError::RootNotDir {
            path: PathBuf::from(r"E:\nope"),
        };
        let msg = e.to_string();
        assert!(msg.contains(r"E:\nope"), "{msg}");
        assert!(msg.contains("目录"), "{msg}");
    }

    #[test]
    fn 无库错误说清成对要求() {
        let msg = AssetError::NoLibraries {
            root: PathBuf::from(r"E:\empty"),
        }
        .to_string();
        assert!(msg.contains("没有登记到任何图库"), "{msg}");
        assert!(msg.contains(".wzl+.wzx"), "{msg}");
        assert!(msg.contains(".wil+.wix"), "{msg}");
    }

    /// 新增的两个 `.lib` 变体必须带路径（与帧号），且措辞能区分「头部坏」与「单帧坏」。
    #[test]
    fn lib错误带路径与帧号() {
        let header = AssetError::LibHeaderInvalid {
            path: PathBuf::from(r"D:\Data\Map\WemadeMir2\Tiles.lib"),
            reason: "不支持的版本号 7".into(),
        };
        let msg = header.to_string();
        assert!(msg.contains("文件头非法"), "{msg}");
        assert!(msg.contains("Tiles.lib"), "{msg}");
        assert!(msg.contains("版本号 7"), "{msg}");

        let frame = AssetError::FrameDecodeFailed {
            path: PathBuf::from(r"D:\Data\Map\WemadeMir2\Objects.lib"),
            index: 42,
            reason: "解压后 16 字节，应为 24 字节".into(),
        };
        let msg = frame.to_string();
        assert!(msg.contains("第 42 帧"), "{msg}");
        assert!(msg.contains("Objects.lib"), "{msg}");
        assert!(msg.contains("应为 24 字节"), "{msg}");
    }

    #[test]
    fn 可当标准错误装箱且能取到source() {
        use std::error::Error;

        let inner = io::Error::new(io::ErrorKind::NotFound, "缺文件");
        let e = AssetError::io("x.wzl", inner);
        assert!(e.source().is_some(), "Io 变体应提供 source()");

        let boxed: Box<dyn Error> = Box::new(AssetError::NoLibraries {
            root: PathBuf::from("r"),
        });
        assert!(boxed.to_string().contains("没有登记到任何图库"));
    }
}
