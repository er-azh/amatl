/// QFixed: 1/64 px.
pub type Fixed = i64;

pub fn to_px(v: Fixed) -> f32 {
    v as f32 / 64.0
}

pub(super) fn from_real(v: f64) -> Fixed {
    (v * 64.0).round() as Fixed
}

pub(super) fn ceil(v: Fixed) -> Fixed {
    (v + 63) & !63
}

pub(super) fn round(v: Fixed) -> Fixed {
    (v + 32) & !63
}
