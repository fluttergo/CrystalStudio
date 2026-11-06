/* =============================================================================
   runtime-config.js —— 容器运行时注入点（默认空）
   仓库里这份是空默认值；docker/entrypoint.sh 会在容器启动时**覆盖**它，
   把 MIREDIT_API_BASE / MIREDIT_MODE 两个环境变量塞进来。
   优先级低于 index.html 里手写的 window.__XXX__ 与 window.__MIREDIT_CONFIG__，
   详见 js/config.js 的 pick() 说明。
   ============================================================================= */
window.__MIREDIT_RUNTIME__ = window.__MIREDIT_RUNTIME__ || {};
