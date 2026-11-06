/* =============================================================================
   edit.js —— 编辑命令队列（所有写操作都走这里）
   · 命令形态 = `miredit_core::Command` 的 JSON 直译（crates/miredit-web/src/json.rs）：
       {"Paint":{"x":12,"y":34}}   {"Erase":{"x":12,"y":34}}
       {"Fill":{"x":12,"y":34}}    {"Pick":{"x":12,"y":34}}
       {"SetLayer":{"layer":"middle"}}   {"SetBrushSize":{"size":5}}
       {"SetTool":{"tool":"eraser"}}     "Undo" / "Redo"
   · 串行发送：一次只有一条在飞，保证「先吸管后落笔」这类顺序不被网络打乱
   · 合并：队列里已有**同一条**命令就不再入队（拖动时同一格只发一次）
   ============================================================================= */
(function (ME) {
  'use strict';

  var queue = [];
  var busy = false;
  var lastResult = null;
  var lastCommand = null;
  var totals = { commands: 0, changed: 0, failed: 0 };

  /**
   * 命令去重键。
   *
   * ⚠ 必须把**整个 body** 算进去：早先只拼 `x,y`，于是
   * `{"SetLayer":{"layer":"middle"}}` 与 `{"SetLayer":{"layer":"front"}}`、
   * `{"SetTool":{"tool":"brush"}}` 与 `{"SetTool":{"tool":"eraser"}}`
   * 会算出同一个键 ⇒ 后一条被当成重复**丢掉**（切层 / 切工具静默失效）。
   *
   * ⚠ 只带 `x`/`y` 两个字段的命令（`Paint`/`Erase`/`Fill`/`Pick`）才用 `x,y` 做键 ——
   * 拖动时同一格重复落笔要被合并。**多字段命令**（`PlaceStamp` 带 `stamp`、
   * `SetSelection` 带四个坐标、`PasteSelection`）必须整份 body 入键，否则
   * 「同一格换一枚印章」会被误判成重复而静默丢掉。
   */
  function keyOf(cmd) {
    if (typeof cmd === 'string') return cmd;
    var tag = Object.keys(cmd)[0];
    var body = cmd[tag];
    if (body && typeof body === 'object') {
      var keys = Object.keys(body);
      var xyOnly = keys.length === 2 && body.x !== undefined && body.y !== undefined;
      if (xyOnly) return tag + ':' + body.x + ',' + body.y;
      return tag + ':' + JSON.stringify(body);
    }
    return tag;
  }

  function enqueue(cmd) {
    var k = keyOf(cmd);
    for (var i = 0; i < queue.length; i++) {
      if (queue[i].key === k) return false;   // 合并重复请求
    }
    queue.push({ cmd: cmd, key: k });
    pump();
    return true;
  }

  async function pump() {
    if (busy) return;
    busy = true;
    while (queue.length) {
      var item = queue.shift();
      lastCommand = item.cmd;
      try {
        var r = await ME.api.edit(item.cmd);
        lastResult = r;
        totals.commands++;
        if (r.changed) totals.changed += r.changed;
        ME.emit('edit:result', { cmd: item.cmd, result: r });
      } catch (e) {
        totals.failed++;
        ME.emit('edit:error', { cmd: item.cmd, error: e });
      }
    }
    busy = false;
    ME.emit('edit:idle', {});
  }

  var edit = {
    /** 落一笔（笔刷按后端当前尺寸，默认 3×3 ⇒ 最多 9 格） */
    paint: function (x, y) { return enqueue({ Paint: { x: x, y: y } }); },
    /**
     * 擦一笔（按后端当前笔刷尺寸；图号层清图号、保留阻挡位）。
     * ⚠ 后端执行 `Erase` 后**工具留在橡皮**，切回画笔必须显式发 [`setTool`]。
     */
    erase: function (x, y) { return enqueue({ Erase: { x: x, y: y } }); },
    /** 连通填充（一次填充 = 一步撤销） */
    fill: function (x, y) { return enqueue({ Fill: { x: x, y: y } }); },
    /** 吸管：把该格当前层取成笔刷，回执 note 里带图号 */
    pick: function (x, y) { return enqueue({ Pick: { x: x, y: y } }); },
    /**
     * 换工具。`toolKey` 只认后端的四个键：`brush` / `bucket` / `eraser` / `pickup`
     * （`ME.TOOLS[*].toolKey` 已把前端词汇翻好，别在这里再传 `fill`/`pick`）。
     */
    setTool: function (toolKey) { return enqueue({ SetTool: { tool: toolKey } }); },
    /** 切当前编辑层：键只认 ground / middle / front / low_collision / high_collision */
    setLayer: function (cmdKey) { return enqueue({ SetLayer: { layer: cmdKey } }); },
    /** 笔刷边长（后端夹进 3..=31 的奇数） */
    setBrushSize: function (size) { return enqueue({ SetBrushSize: { size: size } }); },

    /* ---------- 印章（后端先按 id 读 .X 注册格表，再逐格贴；整次 = 一步撤销） ---------- */
    /**
     * 放一枚**已注册**的印章。
     * 失败口径（后端原文，前端不改写）：id 不在目录 ⇒ 404 `not_found`；
     * 目录不可用 ⇒ 503；`.X` 坏了 ⇒ 422/500；整枚越界 ⇒ 200 + `changed=0` + 中文原因。
     */
    placeStamp: function (x, y, id) {
      return enqueue({ PlaceStamp: { x: x, y: y, stamp: id } });
    },

    /* ---------- 矩形选区 / 剪贴板（七条命令，全是内核变体） ---------- */
    /** 设选区（闭区间；后端会裁剪，回执 note 里带裁剪后的真值） */
    setSelection: function (r) {
      return enqueue({ SetSelection: { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 } });
    },
    /** 取消选区（**不清剪贴板**） */
    clearSelection: function () { return enqueue('ClearSelection'); },
    /** 复制选区进剪贴板（不改图、不置脏） */
    copySelection: function () { return enqueue('CopySelection'); },
    /** 剪切 = 复制 + 清空（一步撤销） */
    cutSelection: function () { return enqueue('CutSelection'); },
    /** 清空选区内容（三族按层清，一步撤销） */
    deleteSelection: function () { return enqueue('DeleteSelection'); },
    /** 用当前笔刷填满选区（没有笔刷 ⇒ changed=0 + 后端原因） */
    fillSelection: function () { return enqueue('FillSelection'); },
    /** 以 (x,y) 为左上角粘贴（越界逐格跳过） */
    pasteSelection: function (x, y) {
      return enqueue({ PasteSelection: { x: x, y: y } });
    },

    undo: function () { return enqueue('Undo'); },
    redo: function () { return enqueue('Redo'); },
    /** 保存走 /api/save（响应带磁盘真实字节数）；不走 Command::Save */
    save: function () {
      ME.debug.push('edit:save', {});
      return ME.api.save();
    },
    get lastResult() { return lastResult; },
    get lastCommand() { return lastCommand; },
    get busy() { return busy; },
    get pending() { return queue.length; },
    totals: totals,
    keyOf: keyOf
  };

  ME.edit = edit;
})(window.ME);
