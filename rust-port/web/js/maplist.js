/* =============================================================================
   maplist.js —— 「打开地图」对话框里的地图列表（GET /api/maps）

   设计要点（对应任务书 §7）：
   1. **一次拉够 + 本地过滤**：`/api/maps` 单页上限 500 条，这里按 500/页把
      `total` 全拉完（61 张 = 1 次请求，1097 张 = 3 次），之后**输入搜索框不再发请求**。
      搜索是纯客户端子串过滤（大小写不敏感，空格分隔多关键词取交集）。
   2. **绝不解析地图内容**：后端只给 `{name,path,bytes,mtime}`，这里也只显示这些。
      大小按 KB/MB 格式化；`mtime` 是 Unix 秒，转本地时间。
   3. **点击一项 = 走现有 open 通路**：只发 `ME.emit('map:open-path', path)`，
      与手打路径走的是同一条（`app.js::openMap` → `POST /api/map/open`）。
   4. **失败不造假**：503 / 网络错误把后端原文上屏，列表留空并写明原因。
   ============================================================================= */
(function (ME) {
  'use strict';

  /** 与后端 `MAX_LIMIT` 一致（超了后端回 400，所以这里不许写更大） */
  var PAGE = 500;
  /** 翻页兜底：条目再多也不拉超过这么多（防止病态目录把浏览器内存吃干） */
  var MAX_ITEMS = 5000;
  /** 一次最多渲染多少行 DOM（1097 行全渲染会拖慢移动端；超出时提示缩小搜索范围） */
  var RENDER_CAP = 300;

  var el = null;        // {dialog, search, refresh, meta, list}
  var onPick = null;    // 选中一项的回调（ui.js 注入：发 map:open-path）
  var all = [];         // 已拉到的全部条目
  var total = 0;        // 后端给的 total（**总数**，不是本页条数）
  var loaded = false;   // 是否已经拉过（拉过就不再请求，除非点「刷新」）
  var inflight = null;  // 在途 Promise（连点两次不会发两遍）

  /* ---------- 纯函数（可在 Node / CDP 里直接断言） ---------- */

  /**
   * 字节数 → 人看的大小。1024 进制（与 Windows 资源管理器同口径）。
   * 取不到（null / NaN / 负数）⇒ `大小未知`，**不写 0 B 冒充空文件**。
   */
  function formatBytes(n) {
    if (typeof n !== 'number' || !isFinite(n) || n < 0) return '大小未知';
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return (n / 1024).toFixed(1) + ' KB';
    return (n / (1024 * 1024)).toFixed(1) + ' MB';
  }

  /** Unix 秒 → `YYYY-MM-DD HH:mm`（本地时区）；取不到 ⇒ 空串 */
  function formatTime(sec) {
    if (typeof sec !== 'number' || !isFinite(sec) || sec <= 0) return '';
    var d = new Date(sec * 1000);
    if (isNaN(d.getTime())) return '';
    var p = function (x) { return (x < 10 ? '0' : '') + x; };
    return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) +
      ' ' + p(d.getHours()) + ':' + p(d.getMinutes());
  }

  /**
   * 客户端过滤：大小写不敏感子串；空格分隔的多个词取**交集**（`n0 map` 两个都得命中）。
   * 同时匹配 `name` 与 `path`（深层目录里同名文件靠路径区分）。
   */
  function filter(items, q) {
    var s = String(q === null || q === undefined ? '' : q).trim().toLowerCase();
    if (!s) return items;
    var parts = s.split(/\s+/);
    return items.filter(function (it) {
      var hay = (String(it.name || '') + ' ' + String(it.path || '')).toLowerCase();
      for (var i = 0; i < parts.length; i++) {
        if (hay.indexOf(parts[i]) < 0) return false;
      }
      return true;
    });
  }

  /* ---------- 拉全量 ---------- */

  /**
   * 拉全量（按 500/页翻到 `total`）。已有缓存且 `force !== true` ⇒ 直接返回缓存。
   * 失败时**不置 loaded**，下次打开对话框会重试。
   */
  function load(force) {
    if (loaded && !force) return Promise.resolve({ items: all, total: total });
    if (inflight) return inflight;
    inflight = (async function () {
      var acc = [];
      var tot = 0;
      var off = 0;
      for (;;) {
        var page = await ME.api.listMaps(off, PAGE);
        if (!page || !Array.isArray(page.items)) {
          throw new ME.ApiError('bad_response', '/api/maps 的响应里没有 items 数组', 0);
        }
        tot = Number(page.total) || 0;
        acc = acc.concat(page.items);
        off += page.items.length;
        // 三种收尾：后端说没有更多了 / 这一页是空的 / 撞到本地兜底上限
        if (page.items.length === 0 || off >= tot || acc.length >= MAX_ITEMS) break;
      }
      all = acc;
      total = tot;
      loaded = true;
      return { items: all, total: total };
    })();
    var p = inflight;
    // 无论成败都要清掉在途标记（失败后允许重试）
    p.then(function () { inflight = null; }, function () { inflight = null; });
    return p;
  }

  /* ---------- 渲染 ---------- */

  function metaText(matched) {
    var s = '共 ' + total + ' 张';
    if (all.length < total) s += '（已加载 ' + all.length + '）';
    if (matched !== all.length) s += ' · 匹配 ' + matched + ' 条';
    if (matched > RENDER_CAP) s += ' · 仅渲染前 ' + RENDER_CAP + ' 条，输入关键词缩小范围';
    return s;
  }

  function render() {
    if (!el) return;
    var matched = filter(all, el.search.value);
    var shown = matched.slice(0, RENDER_CAP);
    var html = '';
    for (var i = 0; i < shown.length; i++) {
      var it = shown[i];
      var t = formatTime(it.mtime);
      html += '<button type="button" class="dialog__item maplist__item" role="option"' +
        ' data-path="' + ME.util.esc(it.path) + '"' +
        ' title="' + ME.util.esc(it.path) + '">' +
        '<b>' + ME.util.esc(it.name) + '</b>' +
        '<span>' + ME.util.esc(formatBytes(it.bytes)) + (t ? ' · ' + ME.util.esc(t) : '') + '</span>' +
        '</button>';
    }
    if (!html) {
      html = '<p class="maplist__empty">' +
        (all.length === 0 ? '这个地图根下一张 .map 都没有。' : '没有匹配「' + ME.util.esc(el.search.value) + '」的地图。') +
        '可用下方「手打路径（退路）」直接填相对路径。</p>';
    }
    el.list.innerHTML = html;
    el.meta.textContent = metaText(matched.length);
  }

  /** 打开一项：先关对话框，再走与手打路径完全相同的 open 通路 */
  function pick(path) {
    if (!path) return;
    if (el && el.dialog && typeof el.dialog.close === 'function') {
      try { el.dialog.close(); } catch (e) { /* 非模态（老浏览器）时忽略 */ }
    }
    if (onPick) onPick(path);
  }

  /* ---------- 对外 ---------- */

  /** 绑定 DOM 与事件（只调一次，由 `ui.init` 调）。`pick` = 选中一项时执行的回调 */
  function mount(elements, pickHandler) {
    el = elements;
    onPick = pickHandler;
    if (!el || !el.list || !el.search) return;

    // 输入即过滤：**纯本地**，一次请求都不发
    el.search.addEventListener('input', render);

    // 回车 = 打开第一条匹配项（没有匹配就什么都不做，别静默提交空路径）
    el.search.addEventListener('keydown', function (ev) {
      if (ev.key !== 'Enter') return;
      ev.preventDefault();
      var first = el.list.querySelector('.maplist__item');
      if (first) pick(first.dataset.path);
      else ME.ui.toast('没有匹配的地图：换个关键词，或用下方「手打路径」');
    });

    // 事件委托：1097 行也只挂一个监听器
    el.list.addEventListener('click', function (ev) {
      var btn = ev.target && ev.target.closest ? ev.target.closest('.maplist__item') : null;
      if (btn) pick(btn.dataset.path);
    });

    if (el.refresh) {
      el.refresh.addEventListener('click', function () {
        loaded = false;
        refresh(true);
      });
    }
  }

  /** 打开对话框时调用：首次拉全量，之后复用缓存（`force` = 点「刷新」） */
  function refresh(force) {
    if (!el || !el.list) return Promise.resolve(null);
    if (el.meta) el.meta.textContent = '正在读取地图列表…';
    el.list.innerHTML = '';
    return load(!!force).then(function () {
      render();
      return { items: all, total: total };
    }, function (e) {
      if (el.meta) el.meta.textContent = '读取失败';
      el.list.innerHTML = '<p class="maplist__error">' + ME.util.esc(ME.api.describe(e)) + '</p>';
      return null;
    });
  }

  ME.mapList = {
    mount: mount,
    refresh: refresh,
    formatBytes: formatBytes,
    formatTime: formatTime,
    filter: filter,
    /** 供 CDP / 自动化断言：当前缓存（只读快照） */
    snapshot: function () {
      return { items: all.slice(), total: total, loaded: loaded };
    }
  };
})(window.ME);
