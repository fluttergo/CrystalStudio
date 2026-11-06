/* =============================================================================
   stampmode.js —— 印章「待放置态」（前端交互层）
   · 进入：印章列表每行的「放置」按钮 / 行被选中后按 Enter / 手机长按该行
   · 待放置时：画布光标变十字 + 顶部提示条「点击画布放置：<印章名>，Esc 取消」
   · 点画布 ⇒ `{"PlaceStamp":{"x","y","stamp":"<id>"}}` ⇒ 后端先按 id 读 .X 注册格表再逐格贴
   · 失败：**后端原文**上顶部错误条（404 id 不存在 / 409 未开图 / 422 .X 坏了 / 503 目录不可用），
     200 但 changed=0（整枚越界、Type5 闸门等）也照原文上条 —— 它同样是「没放成」。
   · Esc 取消待放置态（模式本身留在前端，后端没有「印章工具」这种工具位）
   ============================================================================= */
(function (ME) {
  'use strict';

  var armed = null;          // {id, name}
  var placed = 0;            // 本次待放置态里成功放了几次（只用于 toast，不上提示条）

  function hintText() {
    return '点击画布放置：' + (armed ? armed.name : '') + '，Esc 取消';
  }

  function refresh() {
    if (!armed) { ME.ui.hideMode(); return; }
    ME.ui.showMode('hint', hintText());
  }

  /** 进入待放置态。`id` = `/api/stamps` 的 id（放置命令里的 `stamp` 字段原样用它） */
  function arm(id, name) {
    if (!id) return false;
    armed = { id: String(id), name: name ? String(name) : String(id) };
    placed = 0;
    ME.canvas.setCursor('stamping');
    refresh();
    ME.emit('stamp:armed', armed);
    ME.debug.push('stamp:arm', armed);
    return true;
  }

  /** 取消待放置态；返回 `true` = 之前确实处于待放置态（Esc 用它决定要不要吃掉这次按键） */
  function cancel() {
    if (!armed) return false;
    var was = armed;
    armed = null;
    placed = 0;
    ME.canvas.setCursor(null);
    ME.ui.hideMode();
    ME.emit('stamp:armed', null);
    ME.debug.push('stamp:disarm', { id: was.id });
    return true;
  }

  /**
   * 在某一格放置。返回 `true` = 这次点击已被待放置态接管（app.js 不再落笔）。
   * 点在地图外 ⇒ 直接提示、**不发请求**（后端也会按越界跳过，白跑一趟没意义）。
   */
  function place(cell) {
    if (!armed) return false;
    if (!cell || !cell.inside) {
      ME.ui.toast('点在地图外：请点地图范围内的格子（本次未发送）');
      return true;
    }
    ME.edit.placeStamp(cell.x, cell.y, armed.id);
    ME.debug.push('stamp:place', { x: cell.x, y: cell.y, id: armed.id });
    return true;
  }

  /* ---------- 回执 ---------- */
  function onResult(p) {
    var name = (typeof p.cmd === 'string') ? p.cmd : Object.keys(p.cmd)[0];
    if (name !== 'PlaceStamp') return;
    var r = p.result || {};
    var note = r.note || '';
    if (r.changed > 0) {
      placed++;
      // 成功：提示条保持「待放置」（可以接着放），回执原文进 toast
      if (note) ME.ui.toast(note, 4200);
      refresh();
      ME.debug.push('stamp:ok', { changed: r.changed, note: note, placed: placed });
    } else {
      // 200 + changed=0 = 内核语义拒绝（整枚越界 / 闸门），原因就是后端 note 原文
      ME.ui.showMode('error', '放置印章未落格 · ' + (note || '后端未给出原因'));
      ME.debug.push('stamp:refused', { note: note });
    }
  }

  function onError(p) {
    var name = (typeof p.cmd === 'string') ? p.cmd : Object.keys(p.cmd)[0];
    if (name !== 'PlaceStamp') return;
    var e = p.error || {};
    // 后端原文：describe 只加「错误码中文标签 + 冒号」，message 一个字符都不改写
    ME.ui.showMode('error', '放置印章失败 · ' + ME.api.describe(e));
    ME.debug.push('stamp:error', { code: e.code, message: e.message, status: e.status });
  }

  function init() {
    ME.on('edit:result', onResult);
    ME.on('edit:error', onError);
  }

  ME.stampMode = {
    init: init,
    arm: arm,
    cancel: cancel,
    place: place,
    refresh: refresh,
    hintText: hintText,
    get armed() { return armed; },
    get placedCount() { return placed; }
  };
})(window.ME);
