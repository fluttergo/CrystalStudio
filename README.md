# Crystal Studio

> 仓库名 `CrystalStudio-Rust`；软件显示名 **`Crystal Studio`**（两个词，C/S 大写）。`miredit-*` / `mir2-*` 只是 crate 与二进制的技术标识，不是产品名。

**Crystal Studio** 是传奇 M2 系地图编辑器（上游为 C# / Avalonia 实现）的 **Rust 全量移植**：8 个单向依赖的 crate，PC 原生版与网页版共用同一编辑内核，覆盖 **9 种地图格式**的判别、解析与写回。

上游参照：[`fluttergo/CrystalStudio`](https://github.com/fluttergo/CrystalStudio)，本仓对齐的上游基线提交为 `901dbe2f`。上游 HEAD `6450da90` 仅新增 CellHover + NativeAOT 发布链，未改变地图格式/渲染/资产解码。

## 一、能力

| 项 | 内容 |
|---|---|
| 地图格式 | **9 种**：`Old12` / `Wemade2010` / `Shanda14` / `Shanda2012`（36 B/格）/ `AntiHack` / `Mir3-Wemade` / `Mir3-Shanda` / `Heroes34` / `CrystalCs`（C# 版） |
| 格式保真 | 写回走**原格式**；未建模的尾部/未知区字节原样保留，不猜测语义（带尾块的地图按宽松口径也能打开，并如实标注未解字节数） |
| 渲染 | 三层合成（地表 / 中层 / 物体）、四档缩放（25% / 50% / 100% / 200%）、门帧偏移（开/闭门）、地图光照三档（白天 / 黎明·黄昏 / 夜晚）、低/高阻挡标记 |
| 编辑 | 图层切换、笔刷（3..31 奇数边长）、Bresenham 描线、BFS 连通填充、橡皮擦、吸管、撤销/重做、矩形选区与剪贴板、印章（`.X`）放置、原格式保存 |
| PC 版 | egui 原生窗口，浅色金主题，中文界面；菜单 5 个 / 40 项（含 3 个子菜单）；地图列表面板；网格抽稀；多种诊断与导出开关 |
| 网页版 | axum 后端 + 原生 JS 前端（不引框架 / CDN / npm），桌面与手机浏览器适配，WebSocket 推送脏标记与重绘区域 |
| 导出 | 选区导出、小地图（官方三档预设或指定像素）、1:1 渲染转储 |

## 二、架构（8 crate，依赖单向）

```text
基础层（无内部依赖）
  mir2-map      地图字节 / 单元格模型
  mir2-format   9 种格式的判别与读写（只依赖 std）
  mir2-stamp    .X 印章格式
  mir2-assets   图库解码（Mir2 .wil/.wix、.wzl/.wzx；原版 Crystal .lib）/ 帧锚点

第二层
  miredit-render  合成 / 缩放 / 光照 / 门帧     ← mir2-map, mir2-assets, mir2-stamp

第三层
  miredit-core    编辑内核（命令 / 会话 / 撤销） ← mir2-map, mir2-format, mir2-stamp, mir2-assets

第四层（两者互不依赖）
  miredit-gui     PC 原生版（egui）             ← 上述 + miredit-render, miredit-core
  miredit-web     网页版后端（axum），前端在 web/ ← 上述全部
```

依赖只允许自上向下；`mir2-format` 故意保持零第三方依赖，便于把格式层单独复用与对拍。

## 三、构建与测试

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --all-targets -- -D warnings
```

通过/失败的具体条数以**实际运行结果**为准（`cargo test` 会自己打印），本文件不锁定会过期的数字。

**公开 CI**：[`.github/workflows/ci.yml`](.github/workflows/ci.yml) 在上述同三条命令上跑，且**不设任何素材环境变量** ——
素材相关用例自己打印 `[skip]` / `[跳过]` 后返回，所以「没有游戏素材」也是全绿。CI 不跑 `cargo fmt`、不下载素材、不跑 Docker。

发布构建：

```bash
cargo build -p miredit-gui --release
cargo build -p miredit-web --release
```

> **前置依赖**：WIL/WZL 解码器已 vendored 在 `vendor/wil2atlas/`（decoder-only，MIT；来源与逐文件 SHA256 见 `vendor/wil2atlas/NOTICE`），
> `crates/mir2-assets` 以仓库内相对路径引用它 —— **无需外部 path 依赖**，clone 后可直接构建。
>
> 本项目**不使用 `cargo fmt`**，代码风格靠既有约定与 clippy 维持，见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 四、运行

### 4.1 PC 版

```bash
cargo run -p miredit-gui --release -- --map /path/to/some.map
cargo run -p miredit-gui --release -- --help
```

常用开关：`--map <文件>` 打开指定地图、`--map-list-root <目录>` 指定地图列表根、`--no-window` 配合 `--dump-render` / `--edit-*` 跑无人值守诊断。

### 4.2 网页版

后端配置**全部走环境变量**（没有对应命令行参数）：

```bash
MIREDIT_BIND=127.0.0.1:8080 \
MIREDIT_MAP_ROOT=/path/to/maps \
MIREDIT_CLIENT_ROOT=/path/to/client-root \
MIREDIT_PALETTE_JSON=/path/to/crystalstudio/catalogue/palette.json \
MIREDIT_WEB_ROOT=/path/to/repo/web \
cargo run -p miredit-web --release
```

| 变量 | 作用 | 未设置时 |
|---|---|---|
| `MIREDIT_BIND` | 监听地址（缺省 `0.0.0.0:8080`） | 用缺省值 |
| `MIREDIT_MAP_ROOT` | `/api/map/open` 的白名单根 | 地图端点一律 503（**不猜目录**） |
| `MIREDIT_CLIENT_ROOT` | 图库（客户端）扫描根 | 出图 / 素材端点 503 |
| `MIREDIT_PALETTE_JSON` | 印章目录索引 `palette.json` | `/api/stamps` 503 |
| `MIREDIT_WEB_ROOT` | 静态前端目录（缺省 = 本仓 `web/`，中立回退） | 找不到 ⇒ `GET /` 404（不伪造页面） |

> ⚠ **没有任何内置路径指向某台开发机**（2026-11-06 起）。四个路径变量都**必须显式设置**才
> 能真的服务数据；不设就是"如实不可用"（503/404 + 启动横幅点名缺哪个变量），不会去猜一个
> `GodotClient/maps` 之类的相对目录。接口契约见 [web/README.md](web/README.md)。

### 4.3 PC 版的素材根

PC 版与网页版共用同一套环境变量（**没有内建缺省根**）：

```bash
MIREDIT_CLIENT_ROOT=/path/to/client-root \
cargo run -p miredit-gui --release -- --map /path/to/some.map
```

| 变量 | 作用 | 未设置时 |
|---|---|---|
| `MIREDIT_CLIENT_ROOT` | 图库扫描根（`.wzl/.wzx`、`.wil/.wix`、`.lib`） | 界面/状态栏如实报「资源库装载失败」，图库 0 个（**不 panic**） |
| `MIR2_STAMP_PALETTE` | 印章目录索引 `palette.json` | 印章面板如实报「加载失败」（不静默空表） |


## 五、游戏素材（不随本仓分发）

本仓**只含代码**。地图编辑器需要客户端素材才能出图：

- **地图**：`.map` 文件目录 → 网页版 `MIREDIT_MAP_ROOT`，PC 版 `--map` / `--map-list-root`。
- **图库**：客户端根目录（Mir2 的 `.wil/.wix`、`.wzl/.wzx`，或原版 Crystal `.lib`）→ 网页版与 PC 版**都**用 `MIREDIT_CLIENT_ROOT`（2026-11-06 起 PC 版也走环境变量，不再有内置常量）。
- **印章**：`catalogue/palette.json` 与并列的 `Objects/*.X` → 网页版 `MIREDIT_PALETTE_JSON`，PC 版 `MIR2_STAMP_PALETTE`。

缺素材时各端点/界面如实报错（404 / 503 / 「图库未就绪」），**不会**画占位图冒充成功。

## 六、文档

| 文件 | 内容 |
|---|---|
| [docs/架构蓝图.md](docs/架构蓝图.md) | 分层、模块边界、里程碑 |
| [docs/地图格式规格.md](docs/地图格式规格.md) | 9 种格式的逐字段字节布局与判别/读写口径 |
| [docs/README.md](docs/README.md) | 文档索引 |
| [web/README.md](web/README.md) | 网页版接口契约（端点 / 错误码 / WebSocket / 前端行为） |
| [docker/README.md](docker/README.md) | 容器部署说明 |
| [layout/README-crystalstudio-ui.md](layout/README-crystalstudio-ui.md) | 上游界面真值数据（`crystalstudio-ui.json` / `crystalstudio-strings.json`）的口径 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 贡献流程 |

## 七、说明

- 界面与文档全中文；官方界面译文的 key 与上游 `Localization/Strings.json` 对齐。
- **不许假数据**：格式字节布局、光照 alpha、网格阈值、小地图尺寸等数值一律回源上游；上游确实不可得时才显式标注「原版不可得 ⇒ 自定值」并写明依据。
- 许可证：**MIT**，见 [LICENSE](LICENSE)。
- Docker：`docker/README.md` 描述的是**尚未在 CI 或任何机器上实跑验证**的编排（开发机无 Docker 环境），请以实际构建结果为准。
