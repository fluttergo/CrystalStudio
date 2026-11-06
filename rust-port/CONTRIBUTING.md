# 贡献指南

本仓是 **Crystal Studio** 地图编辑器的 Rust 移植。改动请遵守下面的门禁与边界。

## 一、环境要求

- Rust stable（`edition = "2021"`，workspace 8 个 crate）。
- `crates/mir2-assets` 复用 **vendored** 的 WIL/WZL 解码器：`vendor/wil2atlas/`（decoder-only，MIT；来源与逐文件 SHA256 见 `vendor/wil2atlas/NOTICE`），以仓库内相对路径 `path = "../../vendor/wil2atlas"` 引用 —— **无需外部 path 依赖**。
- 游戏素材（`.map`、`.wil/.wix`、`.wzl/.wzx`、`.lib`、`Objects/*.X`）**不入库**，见第四节。

## 二、构建与门禁

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --all-targets -- -D warnings
```

提交前至少跑一次 `cargo clippy --all-targets -- -D warnings`（要求 0 warning），并在 PR 里贴实际输出。
全量测试耗时较长时可先跑受影响的 crate（`cargo test -p <crate>`），但合入前要跑一次全量。
CI（`.github/workflows/ci.yml`）在同一套命令下跑，**不设任何素材环境变量** ⇒ 素材相关用例全部 `[skip]`。

## 三、代码风格

- **不跑 `cargo fmt`**：本项目明确禁用自动格式化，仓库现有排版即风格基准（缩进、换行、注释位置都按现状延续）。
- 注释与文档**全中文**，标识符保持英文。
- 不引入新依赖除非有明确理由；`mir2-format` 的零第三方依赖是**刻意约束**，不要往里加 serde 之类的库。
- 生产代码不用 `unwrap()` / `expect()` 处理可恢复错误；错误信息要带上下文，不要吞掉。
- 数值（字节布局、常量、阈值、alpha、尺寸）一律回源上游；上游确实不可得时显式标注「原版不可得 ⇒ 自定值」并写明依据，**禁止静默自造数值**。

## 四、素材与本地测试

- **不要提交任何游戏素材**：地图、图库、印章、客户端资源、截图与渲染产物都不进仓库。
- **本仓没有任何内置素材路径**（2026-11-06 起）。素材根全部由环境变量给：

  | 变量 | 给谁用 | 缺省 |
  |---|---|---|
  | `MIREDIT_CLIENT_ROOT` | PC 版 + 网页版：客户端根（`.wzl/.wzx`、`.wil/.wix`、`.lib`） | **无**（未设 ⇒ 图库未就绪，如实报错） |
  | `MIREDIT_MAP_ROOT` | 网页版：`/api/map/open` 白名单根 | **无**（未设 ⇒ 地图端点 503） |
  | `MIREDIT_PALETTE_JSON` | 网页版：印章目录索引 | **无**（未设 ⇒ `/api/stamps` 503） |
  | `MIR2_STAMP_PALETTE` | PC 版：印章目录索引 | **无**（未设 ⇒ 面板报加载失败） |
  | `MIREDIT_WEB_ROOT` | 网页版：静态前端目录 | 本仓 `web/`（中立回退） |

- **测试专用**（验收/逐字节比对才需要，运行时不读）：

  | 变量 | 含义 |
  |---|---|
  | `MIREDIT_ASSET_ROOT` | 地图语料根（用例内相对路径挂在它下面，如 `GodotClient/maps/0.map`） |
  | `MIREDIT_CLIENT_ATLAS_ROOT` | 客户端烘焙图集根（`<库>/<库>_atlas_NN.png` + `_meta.json`） |
  | `MIREDIT_CLIENT_WIL_ROOT` | 旧版 `.wil/.wix` 容器目录 |
  | `MIREDIT_UPSTREAM_ROOT` | 取证根（`Objects/**/*.X`、`catalogue/palette.json`） |

- 依赖素材的用例在根目录缺失时打印 `[skip]` / `[跳过]` 后返回，因此**没有素材也能 `cargo test --workspace` 全绿**
  （CI 就是这个模式，见 `.github/workflows/ci.yml`）。缺素材导致的失败请按上表配好根目录，
  **不要**为了变绿去放宽断言或删除测试。

## 五、提交与 PR

1. Fork 本仓，从 `main` 拉分支（`feat/…`、`fix/…`、`docs/…`）。
2. 一个 PR 只做一件事；涉及多个 crate 时说明依赖顺序。
3. 提交信息写清「改了什么 / 为什么 / 验证方式」，涉及上游对齐时带 `文件:行号@提交号`。
4. 提 PR 前自查：
   - `cargo clippy --all-targets -- -D warnings` 通过；
   - 没有提交 token / 密钥 / 素材 / `target/` 等构建产物；
   - 文档与代码同步（接口契约、格式规格、README）。
5. 行为变更请同时更新 `docs/` 下对应文档；被推翻的结论就地标注「作废 + 原因」，不要删历史结论。

## 六、许可

贡献即视为同意以 [MIT](LICENSE) 许可发布。
