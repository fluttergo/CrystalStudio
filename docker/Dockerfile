# syntax=docker/dockerfile:1
# =============================================================================
# Crystal Studio（网页版）—— 镜像定义
#
# ⚠ 构建上下文必须是**仓库根**，不是 `docker/` 子目录。
#   原因：`crates/mir2-assets/Cargo.toml` 里写着
#       wil2atlas = { path = "../../vendor/wil2atlas" }
#   这是**仓库内相对路径**（vendored 解码器），所以上下文只要含 `crates/` + `vendor/`
#   就能解析，不再依赖"上一级目录里还并列着一个 wil2atlas/"那种迁移前布局。
#
#   所以：compose 里写 `context: ..` + `dockerfile: docker/Dockerfile`；
#   手工构建在**仓库根**执行：
#       docker build -f docker/Dockerfile -t crystalstudio/miredit:dev .
#
# -----------------------------------------------------------------------------
# 三个阶段（**顺序是刻意的**：Docker 默认构建最后一个阶段）
#
#   build    rust:slim-bookworm  ── 编译 miredit-web（唯一真正需要 Rust 的阶段）
#   web      nginx:1.27-alpine   ── 可选前置层：静态页 + /api/ 反代（profile: front）
#   runtime  debian:bookworm-slim── ★ 默认产物：后端二进制 + 静态前端，一个容器全包
#
# 为什么把 runtime 放最后：裸 `docker build` 应该产出「能直接用」的镜像。
# 早期版本把 nginx 阶段放最后，结果裸构建出来的是「只有静态页、没有后端」的镜像，
# 正是本仓最想避免的「镜像能构建、里面却没后端」的假象。
# =============================================================================


# =============================================================================
# 阶段 1：build —— 编译 Rust 后端
# =============================================================================
FROM rust:slim-bookworm AS build

WORKDIR /src

# 只拷构建真正需要的四样：workspace 清单 + 锁文件 + 七个 crate + vendored 解码器。
# （`docker/`、`web/` 只有后面两个阶段用，不往编译阶段搬。）
# `docker/Dockerfile.dockerignore`（BuildKit 会读 `<Dockerfile>.dockerignore`）把
# `target/`、`_upstream/`、`evidence/`、`layout/` 等挡在上下文外。
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY vendor/ ./vendor/

# `CARGO_INCREMENTAL=0` 是本仓硬规矩（增量编译会 panic encode_metadata）。
# **不设** `CARGO_TARGET_DIR`：容器里没有别的盘，让 cargo 用默认的 /src/target。
ENV CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=never

# 只编 `miredit-web`：其它 workspace 成员只参与依赖解析，不编译
# （这样镜像里不会拉进 miredit-gui 的 egui/wgpu 那一大坨）。
# `--locked` 保证用仓库里那份 Cargo.lock；上游改了清单就**明确失败**，不悄悄升版本。
#
# 依赖里没有需要 C 编译器的 crate（已核对：无 cc/ring/openssl-sys/bindgen；
# flate2 走 miniz_oxide 纯 Rust 后端），所以 rust:slim-bookworm 不带 gcc 也够。
RUN cargo build -p miredit-web --release --locked

# 只把「真的有二进制」这件事在构建期钉死：产物缺失就让构建失败，
# 而不是等到 runtime 阶段的 COPY 才报一句难懂的错。
RUN test -x /src/target/release/miredit-web


# =============================================================================
# 阶段 2：web —— 可选前置层（nginx 静态 + /api/ 反代）
#
# 只在 `docker compose --profile front up -d` 时使用；默认编排不启用。
# 存在理由：gzip、静态短缓存、以及把 /api/ 反代到后端而不暴露后端端口。
# =============================================================================
FROM nginx:1.27-alpine AS web

LABEL org.opencontainers.image.title="Crystal Studio（前端前置层）" \
      org.opencontainers.image.description="nginx 静态页 + /api/ 反代到 miredit-web 后端"

# 运行期只需要根证书（healthcheck 用 busybox 自带的 wget，无需额外安装）
RUN apk add --no-cache ca-certificates

# 静态页面（含 tiles/ 真实瓦片夹具）
COPY web/ /usr/share/nginx/html/
# 站点配置：监听 8080；/api/ 的落点由 api-location.inc 决定（默认 503 桩）
COPY docker/nginx-default.conf    /etc/nginx/conf.d/default.conf
COPY docker/api-location.stub.inc /etc/nginx/conf.d/api-location.inc
COPY docker/api-location.stub.inc /etc/nginx/api-location.stub.inc
COPY docker/entrypoint.sh         /usr/local/bin/crystalstudio-entrypoint.sh

# 清掉只对开发有用的生成脚本（镜像里不需要 Python）；并把目录交给 nginx 用户，
# 因为下面要以非 root 运行（nginx 无法写 root 拥有的目录会直接起不来）。
RUN rm -f /usr/share/nginx/html/tiles/gen_tiles.py \
 && chmod +x /usr/local/bin/crystalstudio-entrypoint.sh \
 && chown -R nginx:nginx /usr/share/nginx/html /etc/nginx/conf.d \
 && chown -R nginx:nginx /var/cache/nginx \
 && touch /var/run/nginx.pid && chown nginx:nginx /var/run/nginx.pid

# 非 root 运行（nginx 镜像自带 nginx 用户，uid 101）
USER nginx

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8080/ >/dev/null 2>&1 || exit 1

ENTRYPOINT ["/usr/local/bin/crystalstudio-entrypoint.sh"]


# =============================================================================
# 阶段 3：runtime —— ★ 默认产物：后端 + 静态前端，一个容器全包
#
# 镜像里**自带**：二进制 + 静态前端（web/）。
# 镜像里**不含**（必须挂卷，否则对应端点如实报 503/404，不伪造数据）：
#   /data/maps                    地图（.map）          → /api/map/open
#   /data/client                  图库（.wil/.wzl/.wix/.wzx）→ /api/tiles、/api/palette
#   /data/crystalstudio/catalogue 印章目录索引 palette.json → /api/stamps
#   /data/crystalstudio/Objects   印章 .X（可选：不挂则 cells/bounds 为 null）
# =============================================================================
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="Crystal Studio（后端+前端）" \
      org.opencontainers.image.description="miredit-web（axum HTTP+WS）+ 静态前端；地图/图库/印章目录挂卷"

# ca-certificates：将来若加 https 上游要用；curl：只给 HEALTHCHECK 与容器内排障用。
# 为什么要装 curl 而不是"不写 HEALTHCHECK"：debian:bookworm-slim 里既没有 curl 也没有 wget，
# 而 `sh -c 'exec 3<>/dev/tcp/...'` 依赖 bash（slim 只有 dash）。
# 装 curl（约 +10 MB）换来 `docker compose ps` 能如实显示 healthy，值。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# 非 root 运行。uid/gid 固定 10001，方便挂载目录时对权限（chown 10001:10001）。
# 同时预建好四个挂载点，这样「没挂卷」与「挂了空目录」表现一致（都如实报 503/404）。
RUN groupadd --system --gid 10001 miredit \
 && useradd --system --uid 10001 --gid miredit --home-dir /app --shell /usr/sbin/nologin miredit \
 && mkdir -p /app/web /data/maps /data/client /data/crystalstudio/catalogue /data/crystalstudio/Objects \
 && chown -R miredit:miredit /app /data

COPY --from=build /src/target/release/miredit-web /usr/local/bin/miredit-web

# 静态前端打进镜像：这样**一个容器**就能同时答 `/` 与 `/api/*`，
# 不依赖 nginx 前置层（前置层是可选的，见阶段 2）。
COPY web/ /app/web/

# gen_tiles.py 是开发期生成瓦片夹具的脚本，镜像里不需要 Python
RUN rm -f /app/web/tiles/gen_tiles.py \
 && chmod +x /usr/local/bin/miredit-web \
 && chown -R miredit:miredit /app/web

WORKDIR /app

# 默认配置（都可用 `-e` 覆盖）：
#   MIREDIT_BIND         —— 必须 0.0.0.0 才能被 `-p 8080:8080` 映射到
#   MIREDIT_MAP_ROOT     —— `/api/map/open` 的白名单根
#   MIREDIT_WEB_ROOT     —— 静态前端目录（镜像内已自带）
#   MIREDIT_CLIENT_ROOT  —— 图库根（.wil/.wzl）；不挂载则出图端点 503
#   MIREDIT_PALETTE_JSON —— 印章目录索引。⚠ **dataRoot = 本文件的父目录的父目录**
#                           （mir2-stamp 用 `dataRoot/objectRoot` 定位 .X，
#                            palette.json 里 objectRoot="Objects"），
#                           所以它必须落在 `<dataRoot>/catalogue/palette.json`。
#                           挂 `CrystalStudio/` 到 /data/crystalstudio 即满足。
ENV MIREDIT_BIND=0.0.0.0:8080 \
    MIREDIT_MAP_ROOT=/data/maps \
    MIREDIT_WEB_ROOT=/app/web \
    MIREDIT_CLIENT_ROOT=/data/client \
    MIREDIT_PALETTE_JSON=/data/crystalstudio/catalogue/palette.json

USER 10001:10001

EXPOSE 8080

# 探 /api/health（不是探 /）：`/api/health` 只要进程能应答就 200，
# 而 `/` 依赖 MIREDIT_WEB_ROOT —— 拿 `/` 当健康判据会把「静态目录没挂」误判成「进程挂了」。
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/api/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/miredit-web"]
