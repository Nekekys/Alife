//! Цвета: палитра клеток и устойчивая раскраска линий.

use eframe::egui::Color32;

/// Цвет клетки по её трём пулам массы. Зелёное — растения, красное — падаль,
/// синеватая подложка — питательные вещества.
pub fn cell_color(plant: f32, carrion: f32, nutrient: f32) -> Color32 {
    let r = carrion.clamp(0.0, 1.0);
    let g = plant.clamp(0.0, 1.0);
    let b = nutrient.clamp(0.0, 1.0);
    Color32::from_rgb(
        (r * 230.0) as u8,
        (16.0 + g * 200.0) as u8,
        (22.0 + b * 70.0) as u8,
    )
}

/// Цвет линии: один и тот же номер предка всегда даёт один и тот же оттенок,
/// поэтому родственные особи видны на карте как пятно одного цвета.
pub fn lineage_color(lineage: u64) -> Color32 {
    // Перемешиваем номер, чтобы соседние линии не сливались по тону.
    let mut x = lineage.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    let hue = (x % 10_000) as f32 / 10_000.0;
    hsv(hue, 0.75, 1.0)
}

/// Градиент от холодного к горячему для энергии, возраста и поколения.
pub fn heat(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    hsv(0.62 - 0.62 * t, 0.85 - 0.45 * t, 0.45 + 0.55 * t)
}

pub fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
