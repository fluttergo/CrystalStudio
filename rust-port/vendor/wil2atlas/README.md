# wil2atlas（vendored 子集）—— WIL/WIX 图库解码器

本目录是 **Crystal Studio（传奇 M2 地图编辑器，Rust 移植）** 从作者自研工具链
`wil2atlas` 复制进来的 **decoder-only（只解码）** 源码子集，供 `crates/mir2-assets`
以 path 依赖复用同一份二进制解析实现。

- **这是解码器源码，不是 GodM2 编辑器**：`wil2atlas` 是作者自己的离线资源转换工具，
  与 GodM2（Delphi/Cocos2d 客户端）没有代码共享，只是同一作者、同一批真实素材的产物工具。
- **只含解码**：WIL/WIX（老版）与 WZL/WZX（23 周年客户端等新版）两种容器的
  帧表解析 + 帧解码 + 客户端硬编码调色板。
- **不含**：CLI 入口、图集打包（MaxRects / 固定格子）、索引图导出、meta JSON 结构、批量并行烘焙。
  这些在 `mir2-assets` 的用途里用不到，故不一并 vendor（详见下节「为什么不整仓复制」）。
- **许可证**：MIT，见同目录 [`LICENSE`](LICENSE)；来源与逐文件哈希见 [`NOTICE`](NOTICE)。

## 模块

| 文件 | 职责 |
|---|---|
| `src/wil_decoder.rs` | 老版 `.wil/.wix`：WIX 偏移表、帧头、8bit 索引 / 16bit R5G6B5 / 内嵌 PNG / zlib(PKGD) 四类载荷 |
| `src/wzl_decoder.rs` | 新版 `.wzl/.wzx`：48 字节 WZX 头 + 16 字节帧块头 + zlib 载荷（8bit / 16bit） |
| `src/palette_client.rs` | 客户端硬编码 256 色调色板（Cocos 客户端渲染口径，非 WIL 文件内调色板） |
| `src/image_lib.rs` | `ImageLib` trait：让两种容器走同一条解码管线（`Send + Sync`） |
| `src/lib.rs` | 只做 `pub mod` 转出上述四个模块 |

`wil2atlas` 的 `Cargo.toml` 依赖只剩 `anyhow`（错误链）、`flate2`（zlib）、
`image`（仅 `png`，用于 WIL 帧内嵌 PNG）。原工具全集用到的 `clap` / `rayon` /
`serde` / `serde_json` 只服务 CLI、图集打包与 meta 序列化，本子集不需要。

## 为什么不整仓复制

上游工具目录含 `target/`（约 7.4 GB 构建产物）、`atlas_out/`、`verify_out/` 等大体积
离线产物，以及只服务 CLI 的源码。公开仓库只需要**解码行为**，所以只 vendor 上面 5 个文件
（合计约 50 KB），并让 `mir2-assets` 用仓库内相对路径依赖它 —— 这样公开 PR 既不依赖
构建机上的绝对路径，也不会把一堆二进制产物塞进版本库。

## 使用

```toml
# crates/mir2-assets/Cargo.toml
[dependencies]
wil2atlas = { path = "../../vendor/wil2atlas" }
```

```rust
use wil2atlas::image_lib::ImageLib;
use wil2atlas::palette_client::CLIENT_PALETTE;
use wil2atlas::wil_decoder::{Palette, WilLib};
use wil2atlas::wzl_decoder::WzlLib;
```

## 纪律

- 本子集**不改解码逻辑**；上游有修正时整文件替换并同步 `NOTICE` 的哈希。
- 不在本目录跑 `cargo fmt`（保持与上游逐字节一致，diff 才有意义）。
