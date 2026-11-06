# =============================================================================
# /api/ 的兜底落点：本 nginx 层没配后端反代时的默认应答。
#
# 返回 503 + 结构化 JSON，前端探活失败后会如实显示
#   「后端未连接：/api/health 无响应」
# 而不是画一张假地图。
#
# 默认编排不走本文件：默认服务 `miredit` 是 Rust 后端本身，`/api/*` 由它直接应答。
# 本文件只在 `--profile front`（nginx 前置层）且**没给** MIREDIT_API_UPSTREAM 时生效。
#
# 想接后端：给容器 MIREDIT_API_UPSTREAM=http://miredit:8080，
# entrypoint.sh 会把本文件替换成反代配置（见 docker/README.md）。
# =============================================================================
location /api/ {
    default_type application/json;
    add_header Cache-Control "no-store" always;
    return 503 '{"error":{"code":"no_backend","message":"本 nginx 层未配置 /api/ 反代（MIREDIT_API_UPSTREAM 为空）。默认编排请直接访问 miredit 服务的 8080 端口。"}}';
}
