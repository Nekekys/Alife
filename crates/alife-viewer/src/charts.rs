//! Простые линейные графики. Своя отрисовка вместо внешней библиотеки:
//! нужен только ряд значений во времени, а лишняя зависимость тянула
//! несовместимую версию egui.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};

/// Рисует ряд `ys` по шагам `xs`. Подписи — минимум, максимум и последнее значение.
pub fn line_chart(
    ui: &mut egui::Ui,
    title: &str,
    xs: &[f64],
    ys: &[f64],
    color: Color32,
    height: f32,
) {
    let width = ui.available_width();
    let (response, painter) = ui.allocate_painter(Vec2::new(width, height), Sense::hover());
    let rect = response.rect;
    let bg = ui.visuals().extreme_bg_color;
    painter.rect_filled(rect, 2.0, bg);

    let faint = ui.visuals().weak_text_color();
    let font = FontId::monospace(10.0);

    if ys.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{title}: нет данных"),
            font,
            faint,
        );
        return;
    }

    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &y in ys {
        lo = lo.min(y);
        hi = hi.max(y);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    // Плоский ряд иначе схлопнулся бы в линию на краю.
    if (hi - lo).abs() < 1e-9 {
        lo -= 0.5;
        hi += 0.5;
    }
    let x0 = xs[0];
    let x1 = *xs.last().unwrap();
    let x_span = if (x1 - x0).abs() < 1e-9 { 1.0 } else { x1 - x0 };

    let plot = Rect::from_min_max(
        rect.min + Vec2::new(2.0, 14.0),
        rect.max - Vec2::new(2.0, 2.0),
    );
    let to_screen = |x: f64, y: f64| -> Pos2 {
        let tx = ((x - x0) / x_span) as f32;
        let ty = ((y - lo) / (hi - lo)) as f32;
        Pos2::new(
            plot.min.x + tx * plot.width(),
            plot.max.y - ty * plot.height(),
        )
    };

    let points: Vec<Pos2> = xs.iter().zip(ys).map(|(&x, &y)| to_screen(x, y)).collect();
    painter.add(Shape::line(points, Stroke::new(1.5, color)));

    painter.text(
        rect.min + Vec2::new(4.0, 1.0),
        egui::Align2::LEFT_TOP,
        title,
        font.clone(),
        ui.visuals().text_color(),
    );
    painter.text(
        Pos2::new(rect.max.x - 4.0, rect.min.y + 1.0),
        egui::Align2::RIGHT_TOP,
        format!("{}", fmt(*ys.last().unwrap())),
        font.clone(),
        color,
    );
    painter.text(
        Pos2::new(plot.min.x + 2.0, plot.min.y),
        egui::Align2::LEFT_TOP,
        fmt(hi),
        font.clone(),
        faint,
    );
    painter.text(
        Pos2::new(plot.min.x + 2.0, plot.max.y),
        egui::Align2::LEFT_BOTTOM,
        fmt(lo),
        font,
        faint,
    );
}

fn fmt(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else if a >= 0.1 {
        format!("{v:.3}")
    } else {
        format!("{v:.4}")
    }
}
