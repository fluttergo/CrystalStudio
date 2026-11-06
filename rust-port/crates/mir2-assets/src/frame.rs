//! 帧数据结构与「图库帧源」抽象。

use crate::lib_paths::LibRef;

/// 一帧解码结果：**行优先 RGBA8**（`rgba.len() == width * height * 4`，自顶向下）+ **锚点偏移**。
///
/// 字段与客户端图集 meta 的 `frames[i]` 一一对应：`width/height` 即 `w/h`，
/// `rgba` 即该帧 `region` 里那块像素的原始字节，`anchor` 即 `anchor: [px, py]`。
/// 逐像素比对（验收门 1）比的是前两者；`anchor` 由 `帧锚点与图集meta逐帧一致` 逐帧对账。
///
/// # `anchor` 是什么（三方同源，不是推测）
///
/// | 来源 | 字段 | 出处 |
/// |---|---|---|
/// | 二进制（WIL/WIX） | 帧头 `[4:6] = px i16` / `[6:8] = py i16` | `vendor/wil2atlas/src/wil_decoder.rs:206-209@412fe3e8` |
/// | 二进制（WZL/WZX） | 块头 `[8:10] = px` / `[10:12] = py`（块头前多 4 字节 marker+unused） | `vendor/wil2atlas/src/wzl_decoder.rs:264-269@412fe3e8` |
/// | 参考 C++ 客户端 | `TDXTextureInfo{ u16 nWidth; u16 nHeight; short px; short py; }` | `client/reference_full/Classes/Wil/WIL.h:50-56@412fe3e8` |
/// | 官方 C# 编辑器 | `Sprite(Bitmap, int X, int Y)`，`X/Y` 读自帧记录 `[4:6]`/`[6:8]` | `CrystalStudio/CrystalStudio/Assets.cs:86@901dbe2f(未入库)` |
///
/// 语义 = **图片左上角相对锚点的绘制偏移**（与客户端一致：`DrawSprite(dx + px, dy + py)`，
/// 见 `GodotClient/game/resources/atlas_loader.gd:318-319@412fe3e8` 的同款注释）。
///
/// # 覆盖范围：三条容器路径（`.wil` / `.wzl` / 原版 `.lib`）
///
/// [`crate::LibPaths`] 只登记 `.wzl/.wzx` 与 `.wil/.wix` 两种容器
/// （`lib_paths.rs:257-262`：数据文件只认 `wzl|wil`，索引文件只认 `wzx|wix`），
/// 其它扩展名一律忽略。官方 C# 编辑器读的 `.lib`（Crystal v2/v3、Sanxiu v5/v6）
/// 走**另一条独立路径** [`crate::CrystalLibPaths`] + [`crate::CrystalLibrary`]，
/// 不经过 `wil2atlas`。
///
/// 因此 `anchor` 有两个生产点，都指向**同一个字段**：
///
/// | 生产点 | 来源字段 |
/// |---|---|
/// | [`crate::WilSource::lookup_frame`] | WIL 帧头 `[4:6]` / WZL 块头 `[8:10]`（见上表） |
/// | [`crate::CrystalLibrary::decode_checked`] | `.lib` 帧头 `[4:6]`；Sanxiu 取元数据的 `x/y`（`Assets.cs:86,163@901dbe2f`） |
///
/// `.lib` 的帧头与 WIL **前 8 字节逐字段同构**（`Assets.cs:86@901dbe2f`），
/// 语义同一字段、位置相同，不需要另立字段。
///
/// # ⚠ 消费口径（谁该用、谁不许用）
///
/// - **光罩（地图光照）无条件用**：官方 `MapViewport.Presentation.cs:84-86@901dbe2f(未入库)`
///   `offset = new(sprite.X, sprite.Y)`，不看库号/图号/尺寸 ⇒ `miredit-render` 的
///   `lighting::light_bounds` 直接用本字段；
/// - **三层合成（地表/中层/表层）不许用**：官方画精灵时 `sprite.X/Y` 只在特例下生效
///   （`MapSpriteDrawing.cs:19-31@901dbe2f(未入库)`：blend 库 `14/27/100..198`、图号
///   `2723..2732`、库 `28`），其余走格栅锚点（`compose.rs:5-14` 的规则表）
///   ⇒ 本字段**不得**接进三层合成，否则与官方不一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 帧宽（像素）。
    pub width: u32,
    /// 帧高（像素）。
    pub height: u32,
    /// 行优先 RGBA8 像素，长度 = `width * height * 4`。
    pub rgba: Vec<u8>,
    /// 锚点偏移 `(px, py)`：图片左上角相对锚点的绘制偏移，单位像素（可为负）。
    ///
    /// 与图集 meta 的 `frames[i].anchor = [px, py]` 同源同值；帧头里恒存在（定长字段），
    /// 不随库/帧变化，取不到只会是「该帧无效」而不是「该容器没有这个字段」。
    pub anchor: (i16, i16),
}

impl Default for Frame {
    /// 全空帧：0×0、无像素、锚点 `(0, 0)`。
    ///
    /// 存在的意义是让**别处的 `Frame { … }` 字面量**可以写成
    /// `Frame { width, height, rgba, ..Frame::default() }`，不必逐处补 `anchor`。
    fn default() -> Self {
        Frame {
            width: 0,
            height: 0,
            rgba: Vec::new(),
            anchor: (0, 0),
        }
    }
}

impl Frame {
    /// 像素字节数（`width * height * 4`）。
    pub fn byte_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// 取某像素的 RGBA；越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let s = self.rgba.get(i..i + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }
}

/// 图库帧源抽象：按「族 + 族内索引」定位库，再按「0-based 帧号」取像素。
///
/// 契约（三条必须同时成立）：
///
/// 1. **不 panic**。任何输入（含 `u32::MAX`）都只返回 `Some` / `None`。
/// 2. **不返回假图**。`None` 明确表示「该库不存在 / 帧号越界 / 空帧（WIX 偏移 0 或尺寸越界）」，
///    绝不返回全透明占位图 —— 渲染层据此显示「缺图」而不是黑块。
/// 3. 返回的 RGBA 必须与客户端离线烘焙图集**逐字节一致**（同一解码器 + 同一调色板）。
pub trait SpriteSource {
    /// 取 `lib` 指定的图库的第 `index` 帧（0-based）；不可得返回 `None`。
    ///
    /// `lib` 用 [`LibRef`] 而不是单个整数：三族的索引空间互不相通，
    /// 详见 [`crate::LibPaths::resolve`] 的说明。
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame>;

    /// **只取锚点偏移**（不解码像素）；不可得返回 `None`。
    ///
    /// 语义与 [`Frame::anchor`] 完全一致（同一个帧、同一个字段），但**不产生像素**：
    /// 光罩定位只需要这一对数，为了拿它去解码一整张 RGBA 是纯浪费。
    ///
    /// # 为什么给默认实现
    ///
    /// 默认实现退化成「取帧再读 `anchor`」—— 对任何实现者都成立，且**不破坏**既有实现
    /// （新增方法带默认体，别处的 `impl SpriteSource` 一行都不用改）。
    /// 能拿到帧头元信息的高效实现（如 [`crate::WilSource`]）应覆盖它，走「只读帧头」的路。
    ///
    /// # 三条契约与 [`SpriteSource::frame`] 逐条相同
    ///
    /// 1. **不 panic**（含 `u32::MAX`）；
    /// 2. **不编造**：取不到就 `None`，绝不返回 `(0, 0)` 冒充「这一帧的锚点是零」——
    ///    「锚点确实是 `(0, 0)`」与「取不到」是两件事，调用方要能区分；
    /// 3. 返回的 `(px, py)` 必须等于同一帧 [`SpriteSource::frame`] 的 `anchor`（测试钉住）。
    fn anchor(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        self.frame(lib, index).map(|f| f.anchor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Default` 的锚点必须是 `(0, 0)`，且宽高为 0、无像素。
    ///
    /// 这条钉住的是**别处 19 处 `Frame { … }` 字面量的语义**：它们用
    /// `..Frame::default()` 省略锚点，等于声明「这些夹具的锚点是零」。
    /// 若哪天 `Default` 被改成别的值，三层合成/印章的既有测试会跟着变，必须先红在这里。
    #[test]
    fn 默认帧锚点为零且无像素() {
        let f = Frame::default();
        assert_eq!(f.anchor, (0, 0));
        assert_eq!((f.width, f.height), (0, 0));
        assert!(f.rgba.is_empty());
        assert_eq!(f.byte_len(), 0);
        assert_eq!(f.pixel(0, 0), None);
    }
}
