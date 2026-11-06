//! 九个格式模块。
//!
//! 每个模块一个文件、一种格式，互不依赖（只依赖 [`crate::model`] 的公共骨架）。
//! 注册表顺序在 [`crate::registry`]，与 C# `LegacyMapReader.cs:55-117` 的判别链一致。

pub mod antihack;
pub mod crystal_cs;
pub mod heroes34;
pub mod mir3_shanda;
pub mod mir3_wemade;
pub mod old12;
pub mod shanda14;
pub mod shanda2012;
pub mod wemade2010;
