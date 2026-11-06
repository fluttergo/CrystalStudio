# Crystal Studio · Docker 部署

> ⚠ **现状：镜像从未在 CI 或任何机器上实跑过。** 开发环境没有 Docker（`docker` / `podman` / `nerdctl` 均不存在），
> 因此下面这份编排只做过静态复核（YAML 字段、`COPY` 源、shell 语法、nginx 配置配平），**没有 `docker build` / `docker compose up` 的实测结果**。
> 请把本文当「设计意图 + 部署清单」，首次部署时以实际输出为准；跑通请自行补一份验证记录。

## 1. 组成

一个容器（服务名 `miredit`）里同时装后端与静态前端：

| 组件 | 说明 |
|---|---|
| `miredit-web` | Rust 后端（axum）：HTTP 端点 + 1 个 WebSocket（`/api/events`） |
| 静态前端 | 本仓 `web/`（原生 HTML/CSS/JS，无框架、无 CDN、无 npm） |
| 监听 | 容器内 `0.0.0.0:8080`，宿主默认 `8080` |

后端自己就能答 `GET /` 与 `/api/*`，**不依赖 nginx**；nginx 前置层是可选的第二个服务（`profile: front`）。

## 2. 起服务

```bash
# ① 在 docker 目录里操作（compose.yaml 在这里；下面的相对卷 ./data/... 也相对它解析）
cd docker

# ② 一条命令验证：构建 → 起 → 探活 → 五项断言 → 收工
./verify.sh            # Windows PowerShell：.\verify.ps1

# ③ 长期起服务（验证通过后再做这步）
docker compose up -d --build
```

| 步骤 | 成功标志 |
|---|---|
| `verify.sh` | 最后一行是 `DOCKER_VERIFY: PASS`，退出码 `0` |
| `docker compose up -d --build` | `docker compose ps` 的 STATUS 列出现 `healthy` |
| 浏览器 | 打开 `http://<主机 IP>:8080/` → 三栏编辑器界面 |

`verify.sh` 跑完会 `docker compose down`（除非加 `--keep`），所以第 ③ 步要单独执行。

**首次起之前先把四个目录指过去**（compose 的缺省值是 `docker/data/` 下的**空占位目录**，
首次 `up` 时 Docker 会自动创建 ⇒ 不指也起得来，但数据端点会如实 404/503）：

```bash
export MIREDIT_MAP_DIR=/path/to/maps
export MIREDIT_CLIENT_DIR=/path/to/client-root
# catalogue/ 与 Objects/ 必须并列在同一个 dataRoot 下（palette.json 里 objectRoot="Objects"）
export MIREDIT_CATALOGUE_DIR=/path/to/crystalstudio/catalogue
export MIREDIT_OBJECTS_DIR=/path/to/crystalstudio/Objects
```

## 3. 两个验证脚本

| 脚本 | 平台 | 用法 |
|---|---|---|
| `verify.sh` | Linux / macOS（bash） | 在 `docker/` 下 `./verify.sh`；在仓库根 `./docker/verify.sh`（两种都行：脚本按**自己所在目录**找 `compose.yaml`） |
| `verify.ps1` | Windows（PowerShell 5.1 / 7） | 在 `docker/` 下 `.\verify.ps1`；在仓库根 `.\docker\verify.ps1` |

两者做**完全相同的五项断言**，输出同一行判据 `DOCKER_VERIFY: PASS`：

```text
1. GET  /api/health            → 200 且 body 含 "ok":true
2. POST /api/map/open          → 200 且 body 含 "width"
3. GET  /api/tiles             → 200，PNG 魔数 89504E470D0A1A0A，字节数 > 1000
4. GET  /api/stamps?q=<关键词>  → 200 且 total > 0
5. GET  /                       → 200 且 Content-Type: text/html
```

常用开关：

```bash
./docker/verify.sh --keep              # 验完不停容器（排查用）
./docker/verify.sh --skip-build        # 镜像已存在（无源码的部署目录用这个）
./docker/verify.sh --port 18080        # 宿主端口不是 8080
./docker/verify.sh --map 41.map        # 指定要打开的地图（缺省找 /data/maps 下第一个 .map）
./docker/verify.sh --help
```

脚本**不读源码**：只驱动 `docker compose` + `curl` 打已发布端口，所以可以单独拷到一台只有 `compose.yaml` 的机器上配 `--skip-build` 使用。

## 4. 端口、卷、环境变量

| 项 | 默认值 | 覆盖方式 |
|---|---|---|
| 宿主端口 | `8080` | `MIREDIT_PORT=18080` 或 `verify.sh --port 18080` |
| 容器端口 | `8080` | 固定（改映射即可，别改容器内端口） |
| 地图目录 | `docker/data/maps`（空占位，`up` 时自动建） | `MIREDIT_MAP_DIR` |
| 图库目录（客户端根） | `docker/data/client`（空占位） | `MIREDIT_CLIENT_DIR` |
| 印章索引目录 | `docker/data/crystalstudio/catalogue`（空占位） | `MIREDIT_CATALOGUE_DIR` |
| 印章 `.X` 目录 | `docker/data/crystalstudio/Objects`（空占位） | `MIREDIT_OBJECTS_DIR` |
| 健康检查 | `GET /api/health` 每 30 s | 改 `compose.yaml` 的 `healthcheck` |

容器内环境变量（`Dockerfile` 的 `ENV`，与后端 `crates/miredit-web/src/state.rs` 一一对应）：

| 变量 | 值 | 说明 |
|---|---|---|
| `MIREDIT_BIND` | `0.0.0.0:8080` | 必须 `0.0.0.0` 才能被 `-p` 映射进来 |
| `MIREDIT_MAP_ROOT` | `/data/maps` | `open` 的白名单根（含 `..` 一律拒） |
| `MIREDIT_WEB_ROOT` | `/app/web` | 静态前端目录 |
| `MIREDIT_CLIENT_ROOT` | `/data/client` | 图库扫描根（`.wil/.wix`、`.wzl/.wzx`、Crystal `.lib`） |
| `MIREDIT_PALETTE_JSON` | `/data/crystalstudio/catalogue/palette.json` | 印章索引 |

### 4.1 挂载口径（三条硬约束）

| 宿主目录 | 容器路径 | 谁需要它 | 不挂的结果 |
|---|---|---|---|
| `MIREDIT_MAP_DIR` | `/data/maps`（`:ro`） | `/api/map/open` | 404 `not_found` |
| `MIREDIT_CLIENT_DIR` | `/data/client`（`:ro`） | `/api/tiles`、`/api/palette` | 503 `assets_unavailable`「图库未就绪」 |
| `MIREDIT_CATALOGUE_DIR` | `/data/crystalstudio/catalogue`（`:ro`） | `/api/stamps` | 503「印章目录不可用」 |
| `MIREDIT_OBJECTS_DIR` | `/data/crystalstudio/Objects`（`:ro`） | 印章 `.X` 解析与放置 | `/api/stamps` 仍 200，但 `cells` / `bounds` 全 `null`，印章只能浏览不能放置 |

1. **`catalogue` / `Objects` 必须并列在同一个 `dataRoot` 下**（`/data/crystalstudio/`）。
   原因：`mir2-stamp` 用 `dataRoot + objectRoot` 定位 `.X`，而 `palette.json` 里写的是 `"objectRoot": "Objects"`。
   挂到 `/data/client/catalogue` 的后果：列表仍 200，但每条 `cells` / `bounds` 都是 `null`。
2. **除地图卷外全部只读（`:ro`）**：容器内 `/api/save` 会因只读挂载失败（`io_error`）。要能保存请去掉地图卷的 `:ro`，并确认容器用户（uid 10001）对宿主目录有写权限。
3. **宿主目录不存在时 Docker 会建一个空目录**（不报错）⇒ 端点如实返回 404 / 503，不会崩溃。

### 4.2 什么都不挂时

镜像本身可独立运行：`/api/health` 与静态页正常，四个数据端点分别 404 / 503。这符合「取不到就如实报错，不画假地图」的口径。

## 5. 可选：nginx 前置层

默认编排**不用** nginx。需要 gzip / 静态短缓存 / 不暴露后端端口时：

```bash
docker compose --profile front up -d --build
# 然后访问 http://<主机 IP>:8081/   （MIREDIT_FRONT_PORT 可改）
```

此时 nginx（容器 8080 → 宿主 8081）把 `/api/` 反代到 `miredit:8080`：

- `compose.yaml` 的 `front.environment` 里配了反代上游地址（`http://miredit:8080`，变量名见该文件的 `MIREDIT_API_*`）；
- `entrypoint.sh` 据此生成 `location /api/ { proxy_pass <upstream>; … }`（未配时落到 503 JSON 桩）；
- `depends_on: miredit: condition: service_healthy` —— nginx 在配置加载阶段就要解析 `miredit` 主机名，解析不到会直接退出（不是「稍后重试」）。

> ⚠ `proxy_pass` **结尾不能带 `/`**：带斜杠会把 `/api/health` 改写成 `/health` 转发，全部 404。这条已在 `nginx-default.conf` 注释标明。

## 6. 文件清单

| 文件 | 作用 |
|---|---|
| `Dockerfile` | 三阶段：`build`（rust 编译）→ `web`（nginx 前置层，可选）→ `runtime`（**默认产物**：后端 + 前端，非 root uid 10001） |
| `Dockerfile.dockerignore` | 把无关大目录挡在构建上下文外（**只有 BuildKit 会读**） |
| `compose.yaml` | 服务 `miredit`（默认）+ 服务 `front`（`profile: front`） |
| `nginx-default.conf` | nginx 站点配置：监听 8080、gzip、静态短缓存、`/api/` 由 include 决定 |
| `api-location.stub.inc` | `/api/` 的 503 JSON 兜底（未配反代时） |
| `entrypoint.sh` | 生成运行时配置、可选开反代、回退到 503 桩 |
| `verify.sh` / `verify.ps1` | 一条命令验证（Linux / Windows） |

## 7. 已知未完成项（部署前必须处理）

| 项 | 状态 | 说明 |
|---|---|---|
| `compose.yaml` 的构建上下文 | ✅ **已解决（2026-11-06）** | 已改为 **仓库根** `context: ..` + `dockerfile: docker/Dockerfile`；`Dockerfile` 的 `COPY` 也改为仓库内相对路径（`Cargo.toml` / `Cargo.lock` / `crates/` / `vendor/` / `web/`）。独立 clone 后目录名叫 `CrystalStudio-Rust` 还是别的都不影响 |
| `mir2-assets` 的 `wil2atlas` 依赖 | ✅ **已解决** | WIL/WZL 解码器已 vendored 到 `vendor/wil2atlas/`（decoder-only，MIT；来源与逐文件 SHA256 见 `vendor/wil2atlas/NOTICE`），`crates/mir2-assets/Cargo.toml` 已改用仓库内相对路径 `path = "../../vendor/wil2atlas"`，不再依赖开发机绝对路径 |
| `docker build` 能成功 | ❌ 未验证 | 无 Docker 环境 |
| Linux 二进制能跑（glibc / 依赖） | ❌ 未验证 | 只做过 Windows 二进制的运行时等价验证 |
| `nginx -t` 能接受这份配置 | ❌ 未验证 | 只做了静态配平与指令拼写核对 |
| `entrypoint.sh` 在 busybox ash 下能跑 | ❌ 未验证 | 只做了 `bash -n`；已确认只用 POSIX 构造 |
| 非 root（uid 10001）对挂载目录的读权限 | ❌ 未验证 | 取决于宿主目录权限，必要时 `chown` / `chmod` |
| 容器内 `/api/save` 写盘 | ❌ 未验证 | 地图卷默认 `:ro` ⇒ 保存必然失败（有意为之，先只读验渲染） |
