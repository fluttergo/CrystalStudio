#!/usr/bin/env bash
# =============================================================================
# Crystal Studio —— Docker 部署「一条命令」验证
#
#   ./verify.sh                # 构建 → 起 → 验 → 收工
#   ./verify.sh --keep         # 验完不停容器（排查用）
#   ./verify.sh --skip-build   # 镜像已构建好（无源码的部署目录用这个）
#
# 成功时**只认这一行**：   DOCKER_VERIFY: PASS
# 失败时打印原因 + 退出码 1。
#
# ── 它验什么（5 项，全绿才算 PASS）────────────────────────────────────────────
#   1. GET  /api/health            → 200 且 body 含 "ok":true
#   2. POST /api/map/open          → 200 且 body 含 "width"
#   3. GET  /api/tiles             → 200，PNG 魔数，字节数 > 1000
#   4. GET  /api/stamps?q=market   → 200 且 total > 0
#   5. GET  /                       → 200 且 Content-Type 是 text/html
#
# ── 前置条件 ─────────────────────────────────────────────────────────────────
#   * 有 docker + compose v2（`docker compose version` 能跑）
#   * 有 curl（fnOS / macOS / 各发行版默认都有）
#   * 地图目录里至少有 1 个 .map（否则第 2 项无从验起 —— 脚本会自己找）
#
# ── 为什么脚本不读源码 ───────────────────────────────────────────────────────
# 它只做两件事：驱动 `docker compose`，以及 `curl` 打已发布的端口。
# 所以你可以把本文件单独拷到一台只有 compose.yaml 的机器上，
# 配 `--skip-build`（镜像已存在）照样能验。
# =============================================================================
set -euo pipefail

# ---------------------------------------------------------------- 默认参数 ---
DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
FILE="compose.yaml"
SERVICE="miredit"
PORT="${MIREDIT_PORT:-8080}"
TIMEOUT=60
KEEP=0
SKIP_BUILD=0
MAP_FILE="${MIREDIT_MAP_FILE:-}"
TILE_W="${MIREDIT_TILE_W:-16}"
TILE_H="${MIREDIT_TILE_H:-16}"

# ------------------------------------------------------------------ 输出 -----
if [ -t 1 ]; then
  C_RED=$'\033[31m'; C_GRN=$'\033[32m'; C_YEL=$'\033[33m'; C_OFF=$'\033[0m'
else
  C_RED=''; C_GRN=''; C_YEL=''; C_OFF=''
fi
say()  { printf '%s\n' "$*"; }
ok()   { printf '%s[ OK ]%s %s\n' "$C_GRN" "$C_OFF" "$*"; }
bad()  { printf '%s[FAIL]%s %s\n' "$C_RED" "$C_OFF" "$*"; }
warn() { printf '%s[WARN]%s %s\n' "$C_YEL" "$C_OFF" "$*"; }
die()  { bad "$*"; say ""; say "DOCKER_VERIFY: FAIL"; exit 1; }

usage() {
  cat <<'EOF'
用法： ./verify.sh [选项]

  构建 → 起 → 轮询 /api/health → 四项端点断言 → 收工（除非 --keep）。
  成功时只认这一行：   DOCKER_VERIFY: PASS

选项：
  -d, --dir <目录>      compose 项目目录（默认：本脚本所在目录）
  -f, --file <文件>     compose 文件名（默认 compose.yaml）
  -p, --port <端口>     宿主端口（默认 8080，或环境变量 MIREDIT_PORT）
      --service <名>    服务名（默认 miredit）
      --timeout <秒>    健康轮询上限（默认 60）
      --map <文件名>    /data/maps 下要打开的地图（默认自动找第一个 .map）
      --keep            验证结束后不停容器
      --skip-build      跳过 docker compose build（镜像已存在时用）
  -h, --help            显示本帮助

环境变量：
  MIREDIT_PORT          宿主端口（等价于 --port）
  MIREDIT_MAP_FILE      要打开的地图（等价于 --map）
  MIREDIT_TILE_W/H      /api/tiles 的取样窗口（默认 16×16；
                        (0,0) 起若是空白区域导致 PNG ≤1000 字节，改成 64×48）

退出码：0 = PASS，1 = FAIL（失败原因打印在 DOCKER_VERIFY: FAIL 之前）
EOF
}

# ------------------------------------------------------------- 参数解析 ------
while [ "$#" -gt 0 ]; do
  case "$1" in
    -d|--dir)      DIR="${2:?--dir 需要值}"; shift 2 ;;
    -f|--file)     FILE="${2:?--file 需要值}"; shift 2 ;;
    -p|--port)     PORT="${2:?--port 需要值}"; shift 2 ;;
    --service)     SERVICE="${2:?--service 需要值}"; shift 2 ;;
    --timeout)     TIMEOUT="${2:?--timeout 需要值}"; shift 2 ;;
    --map)         MAP_FILE="${2:?--map 需要值}"; shift 2 ;;
    --keep)        KEEP=1; shift ;;
    --skip-build)  SKIP_BUILD=1; shift ;;
    -h|--help)     usage; exit 0 ;;
    *)             die "不认识的参数：$1（用 --help 看用法）" ;;
  esac
done

BASE="http://127.0.0.1:${PORT}"
TMP="$(mktemp -d 2>/dev/null || mktemp -d -t mireditverify)"
trap 'rm -rf "$TMP"' EXIT

# --------------------------------------------------------- 0. 前置检查 -------
command -v docker >/dev/null 2>&1 || die "找不到 docker（本机没装 Docker？）"
docker compose version >/dev/null 2>&1 || die "docker compose v2 不可用（试 \`docker compose version\`）"
command -v curl   >/dev/null 2>&1 || die "找不到 curl（本脚本用 curl 打端点；请先安装 curl）"
[ -d "$DIR" ] || die "目录不存在：$DIR"
[ -f "$DIR/$FILE" ] || die "找不到 compose 文件：$DIR/$FILE"
cd "$DIR"

say "=============================================================="
say " Crystal Studio · Docker 部署验证"
say " 项目目录 : $DIR"
say " compose  : $FILE（服务 $SERVICE）"
say " 访问地址 : $BASE"
say "=============================================================="

# ----------------------------------------------------------- 1. 构建 ---------
if [ "$SKIP_BUILD" -eq 1 ]; then
  say "[1/5] 跳过构建（--skip-build）"
else
  say "[1/5] docker compose build ..."
  if ! docker compose -f "$FILE" build; then
    die "构建失败。常见原因：构建上下文不是 tools/（见 docker/README.md §6）、或 Cargo.lock 与清单不同步（--locked）"
  fi
  ok "构建完成"
fi

# ------------------------------------------------------------- 2. 起 ---------
say "[2/5] docker compose up -d ..."
if ! docker compose -f "$FILE" up -d; then
  die "启动失败。看日志：docker compose -f $DIR/$FILE logs --tail=100 $SERVICE"
fi
ok "容器已启动"

# ------------------------------------------------- 3. 轮询 /api/health -------
say "[3/5] 轮询 $BASE/api/health（上限 ${TIMEOUT}s）..."
deadline=$(( $(date +%s) + TIMEOUT ))
healthy=0
last_body=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  if last_body="$(curl -fsS --max-time 3 "$BASE/api/health" 2>/dev/null)"; then
    case "$last_body" in
      *'"ok":true'*) healthy=1; break ;;
      *) : ;;   # 起来了但 body 不对，继续等
    esac
  fi
  sleep 1
done

if [ "$healthy" -ne 1 ]; then
  bad "健康检查在 ${TIMEOUT}s 内没通过（最后响应：${last_body:-<无>}）"
  say ""
  say "---- 容器状态 ----"
  docker compose -f "$FILE" ps || true
  say ""
  say "---- 最近 60 行日志 ----"
  docker compose -f "$FILE" logs --tail=60 "$SERVICE" || true
  say ""
  say "诊断提示："
  say "  * 端口被占？  改 --port 或用 MIREDIT_PORT=18080 重跑"
  say "  * 进程起不来？ 上面日志里应有 [启动失败] 一行"
  say "  * 想看现场？  加 --keep 重跑，然后 docker compose logs -f $SERVICE"
  die "健康检查未通过"
fi
ok "/api/health 200 → $last_body"

# ---------------------------------------- 4. 找出要打开的地图文件 ------------
# 容器内路径 /data/maps/<名字>；脚本只传文件名，后端会拼到 MIREDIT_MAP_ROOT 下。
# 容器里只做 `ls`，取第一条在**宿主侧**做（管道/重定向都不进容器命令：
# 少一层 shell 语义，也避免 Windows 上 cmd 把 `|` 当管道重新解析）
if [ -z "$MAP_FILE" ]; then
  MAP_FILE="$(docker compose -f "$FILE" exec -T "$SERVICE" \
      sh -c 'ls /data/maps/*.map' 2>/dev/null \
      | tr -d '\r' | sed 's#.*/##' | head -n 1 || true)"
fi
if [ -z "$MAP_FILE" ]; then
  MAP_FILE="0.map"
  warn "没在 /data/maps 下找到 .map，回退试 $MAP_FILE"
fi
say "      将打开地图：$MAP_FILE"

# ------------------------------------------------- 5. 四项端点断言 -----------
say "[4/5] 断言端点 ..."

# ---- ① POST /api/map/open ------------------------------------------------
# 只对 " 和 \ 做转义（地图名是文件名，不会有别的需要转义的字符）
esc="${MAP_FILE//\\/\\\\}"; esc="${esc//\"/\\\"}"
# curl 连不上时仍会经 -w 打出 000 并以非 0 退出 ⇒ 用 `|| true` 兜住 set -e，
# 再把空输出补成 000（不要写 `|| echo 000`：那会和 -w 的 000 拼成 000000）
code="$(curl -sS --max-time 30 -o "$TMP/open.json" -w '%{http_code}' \
        -X POST -H 'content-type: application/json' \
        --data-binary "{\"path\":\"$esc\"}" "$BASE/api/map/open" 2>/dev/null || true)"
[ -n "$code" ] || code=000
body="$(cat "$TMP/open.json" 2>/dev/null || true)"
if [ "$code" != "200" ]; then
  bad "POST /api/map/open → HTTP $code"
  say "      $body"
  say "      诊断：/data/maps 里没有 $MAP_FILE？用 --map <文件名> 指定；"
  say "            或 MIREDIT_MAP_DIR 指向正确的地图目录（见 docker/README.md §5）"
  die "map/open 未通过"
fi
case "$body" in
  *'"width"'*) ok "POST /api/map/open 200 且含 width → $(printf '%s' "$body" | cut -c1-160)..." ;;
  *) bad "POST /api/map/open 200 但 body 里没有 width：$body"; die "map/open 响应格式不符" ;;
esac

# ---- ② GET /api/tiles ----------------------------------------------------
code="$(curl -sS --max-time 60 -o "$TMP/tiles.png" -w '%{http_code}' \
        "$BASE/api/tiles?x=0&y=0&w=${TILE_W}&h=${TILE_H}" 2>/dev/null || true)"
[ -n "$code" ] || code=000
size="$(wc -c < "$TMP/tiles.png" 2>/dev/null | tr -d ' ' || echo 0)"
if [ "$code" != "200" ]; then
  bad "GET /api/tiles → HTTP $code"
  say "      $(head -c 400 "$TMP/tiles.png" 2>/dev/null || true)"
  say "      诊断：503 通常是图库没挂上 —— MIREDIT_CLIENT_DIR 要指向"
  say "            「含 .wil/.wzl 的客户端根」（本仓 = client/热血传奇23周年客户端）"
  die "tiles 未通过"
fi
# PNG 魔数：89 50 4E 47 0D 0A 1A 0A
magic="$(od -An -tx1 -N8 "$TMP/tiles.png" 2>/dev/null | tr -d ' \n' || true)"
if [ "$magic" != "89504e470d0a1a0a" ]; then
  bad "GET /api/tiles 返回的不是 PNG（前 8 字节 = ${magic:-<空>}）"
  die "tiles 不是 PNG"
fi
if [ "$size" -le 1000 ]; then
  bad "GET /api/tiles 的 PNG 只有 ${size} 字节（≤1000，疑似空白区域）"
  say "      诊断：地图 (0,0) 起 ${TILE_W}×${TILE_H} 格可能是空的。"
  say "            换参数：MIREDIT_TILE_W=64 MIREDIT_TILE_H=48 ./verify.sh"
  die "tiles 太小"
fi
ok "GET /api/tiles 200，PNG，${size} 字节"

# ---- ③ GET /api/stamps?q=market -----------------------------------------
code="$(curl -sS --max-time 60 -o "$TMP/stamps.json" -w '%{http_code}' \
        "$BASE/api/stamps?q=market" 2>/dev/null || true)"
[ -n "$code" ] || code=000
body="$(cat "$TMP/stamps.json" 2>/dev/null || true)"
if [ "$code" != "200" ]; then
  bad "GET /api/stamps?q=market → HTTP $code"
  say "      $(printf '%s' "$body" | cut -c1-400)"
  say "      诊断：503 通常是印章目录没挂上 —— MIREDIT_CATALOGUE_DIR 要指向"
  say "            CrystalStudio/catalogue（palette.json 就在里面）"
  die "stamps 未通过"
fi
total="$(printf '%s' "$body" | grep -o '"total"[[:space:]]*:[[:space:]]*[0-9]\+' | grep -o '[0-9]\+$' | head -n 1 || true)"
if [ -z "$total" ]; then
  bad "GET /api/stamps 的响应里找不到 total：$(printf '%s' "$body" | cut -c1-300)"
  die "stamps 响应格式不符"
fi
if [ "$total" -le 0 ]; then
  bad "GET /api/stamps?q=market 的 total = 0（印章目录里没有 market 条目）"
  die "stamps 无匹配"
fi
ok "GET /api/stamps?q=market 200，total = $total"

# ---- ④ GET / （静态页）---------------------------------------------------
code="$(curl -sS --max-time 15 -o "$TMP/index.html" -w '%{http_code}' "$BASE/" 2>/dev/null || true)"
[ -n "$code" ] || code=000
ctype="$(curl -sS --max-time 15 -o /dev/null -w '%{content_type}' "$BASE/" 2>/dev/null || true)"
if [ "$code" != "200" ]; then
  bad "GET / → HTTP $code（静态前端没进镜像？MIREDIT_WEB_ROOT 应为 /app/web）"
  die "静态页未通过"
fi
case "$ctype" in
  text/html*) ok "GET / 200，Content-Type=$ctype，$(wc -c < "$TMP/index.html" | tr -d ' ') 字节" ;;
  *) bad "GET / 200 但 Content-Type=$ctype（期望 text/html）"; die "静态页 MIME 不符" ;;
esac

say "[5/5] 全部断言通过"

# ----------------------------------------------------------- 6. 收工 ---------
if [ "$KEEP" -eq 1 ]; then
  say ""
  say "容器保留运行中（--keep）。收工：docker compose -f $DIR/$FILE down"
else
  say "docker compose down ..."
  docker compose -f "$FILE" down >/dev/null 2>&1 || warn "down 失败（容器可能已自行退出）"
fi

say ""
say "DOCKER_VERIFY: PASS"
