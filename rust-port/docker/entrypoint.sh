#!/bin/sh
# =============================================================================
# Crystal Studio —— nginx 前置层的容器启动脚本（compose profile: front）
#
# ⚠ 默认编排不走这里：默认服务 `miredit` 用 Dockerfile 的 `runtime` 阶段，
#   后端自己就能答 `/` 与 `/api/*`。本脚本只服务可选的前置层镜像（阶段 `web`）。
#
# 做三件事，然后前台起 nginx：
#   1. 按环境变量生成浏览器侧运行时配置（runtime-config.js）
#   2. 若给了 MIREDIT_API_UPSTREAM，把 /api/ 换成反代；否则保持 503 桩
#   3. nginx -t 校验；失败则回退到 503 桩，再失败就退出（不假装启动成功）
#
# 环境变量（全部有默认值，未设也能起）：
#   MIREDIT_API_BASE      浏览器侧后端根（空 = 同源），例：http://127.0.0.1:8099
#   MIREDIT_MODE          auto | api | static
#   MIREDIT_API_UPSTREAM  容器侧反代目标，例：http://miredit:8080
# =============================================================================
set -eu

# pipefail：本脚本目前没有任何管道，开它只是为了将来加管道时不丢错误码。
# 为什么用探测而不是直接写 `set -euo pipefail`：
#   shebang 是 /bin/sh（nginx:1.27-alpine 里 = busybox ash），老版本 ash 不认
#   `pipefail` 这个选项名会**直接拒绝启动**（不是警告）。探测失败就跳过，
#   宁可少开一个用不上的选项，也不让容器起不来。
if (set -o pipefail) 2>/dev/null; then
  set -o pipefail
fi

WEB_ROOT=/usr/share/nginx/html
CONF_INC=/etc/nginx/conf.d/api-location.inc
STUB=/etc/nginx/api-location.stub.inc

API_BASE="${MIREDIT_API_BASE:-}"
MODE="${MIREDIT_MODE:-auto}"
UPSTREAM="${MIREDIT_API_UPSTREAM:-}"

echo "[crystalstudio-front] nginx 前置层启动"

# 直接给了命令就跑命令（方便 docker run --rm 镜像 sh 进去排查）
if [ "$#" -gt 0 ]; then
  exec "$@"
fi

# ---- 0. 参数消毒：值里有引号/反斜杠会破坏生成的 JS 或 nginx 配置 ----------
case "${API_BASE}${MODE}${UPSTREAM}" in
  *'"'*|*"\\"*)
    echo "[crystalstudio][警告] 环境变量里含双引号或反斜杠，已全部忽略，回退默认值" >&2
    API_BASE=""; MODE="auto"; UPSTREAM=""
    ;;
esac

case "$MODE" in
  auto|api|static) ;;
  *) echo "[crystalstudio][警告] MIREDIT_MODE=$MODE 不认识，回退 auto" >&2; MODE=auto ;;
esac

# ---- 1. 浏览器侧运行时配置 ------------------------------------------------
if [ -w "$WEB_ROOT" ]; then
  cat > "$WEB_ROOT/runtime-config.js" <<EOF
/* 由 docker/entrypoint.sh 在容器启动时生成，每次启动覆盖；仓库里那份是空默认值 */
window.__MIREDIT_RUNTIME__ = { apiBase: "${API_BASE}", mode: "${MODE}" };
EOF
  echo "[crystalstudio] 运行时配置：apiBase='${API_BASE}' mode='${MODE}'"
else
  echo "[crystalstudio][警告] $WEB_ROOT 不可写，跳过 runtime-config.js（沿用镜像内的默认值）" >&2
fi

# ---- 2. /api/ 落点 --------------------------------------------------------
if [ -n "$UPSTREAM" ]; then
  if [ -w "$CONF_INC" ]; then
    cat > "$CONF_INC" <<EOF
# 由 docker/entrypoint.sh 生成：/api/ 反代到 ${UPSTREAM}
location /api/ {
    proxy_pass ${UPSTREAM};
    proxy_http_version 1.1;
    proxy_set_header Host \$host;
    proxy_set_header X-Real-IP \$remote_addr;
    proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
    proxy_set_header Upgrade \$http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 3600s;
    proxy_buffering off;
}
EOF
    echo "[crystalstudio] /api/ 反代 → ${UPSTREAM}"
  else
    echo "[crystalstudio][警告] $CONF_INC 不可写，无法启用反代，保持 503 桩" >&2
  fi
else
  echo "[crystalstudio] /api/ 未配置反代 → 保持 503 桩（页面会显示「后端未连接」）"
fi

# ---- 3. 校验并启动 --------------------------------------------------------
if ! nginx -t >/tmp/nginx-test.log 2>&1; then
  echo "[crystalstudio][错误] nginx 配置校验失败，回退到 503 桩：" >&2
  cat /tmp/nginx-test.log >&2 || true
  cp "$STUB" "$CONF_INC"
  if ! nginx -t >/tmp/nginx-test.log 2>&1; then
    echo "[crystalstudio][致命] 回退后仍然失败，退出（不假装启动成功）：" >&2
    cat /tmp/nginx-test.log >&2 || true
    exit 1
  fi
  echo "[crystalstudio] 已回退到 503 桩"
fi

echo "[crystalstudio] 监听 :8080"
exec nginx -g 'daemon off;'
