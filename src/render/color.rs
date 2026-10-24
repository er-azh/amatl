use crate::format::PenColor;

impl PenColor {
    pub fn to_argb(self, custom: Option<u32>) -> u32 {
        use PenColor::*;
        match self {
            Black => 0xff000000,
            Gray => 0xff909090,
            White => 0xffffffff,
            HighlighterYellow => 0xfffefd60,
            HighlighterGreen => 0xffa9fa5c,
            HighlighterPink => 0xffff55cf,
            Blue => 0xff304ae0,
            Red => 0xffc23132,
            HighlighterGray => 0x40000000,
            ArgbCode => custom.unwrap_or(0xff000000),
            Green => 0xff91da71,
            Cyan => 0xff74d2e8,
            Magenta => 0xffc07fd2,
            Yellow => 0xfffae719,
            Other(_) => 0xff000000,
        }
    }
}

pub fn premultiply(c: u32) -> u32 {
    if c >= 0xff000000 {
        return c;
    }
    let a = c >> 24;
    (c & 0xff000000)
        | (((a * (c & 0xff0000)) / 0xff) & 0xff0000)
        | (((a * (c & 0xff00)) / 0xff) & 0xff00)
        | ((a * (c & 0xff)) / 0xff)
}

#[inline]
pub fn unpremultiply(c: u32) -> [f32; 3] {
    let a = c >> 24;
    let ch = |s: u32| {
        let v = (c >> s) & 0xff;
        (v * 255)
            .checked_div(a)
            .map_or(0.0, |v| v.min(255) as f32 / 255.0)
    };
    [ch(16), ch(8), ch(0)]
}

/// Straight RGBA bytes, rounding to nearest (unlike `unpremultiply`, which truncates).
#[inline]
pub fn unpremultiply_rgba(c: u32) -> [u8; 4] {
    let a = c >> 24;
    let ch = |s: u32| {
        let v = (c >> s) & 0xff;
        (v * 255 + a / 2)
            .checked_div(a)
            .map_or(0, |v| v.min(255) as u8)
    };
    [ch(16), ch(8), ch(0), a as u8]
}
