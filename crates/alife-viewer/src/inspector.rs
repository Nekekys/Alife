//! Инспектор особи: что она видит и что решает.
//!
//! Без этой панели тихий баг в сенсорах не отличить от «эволюция просто
//! идёт медленно»: по одним числам популяции такое не видно.

use alife_core::brain::OUTPUT_NAMES;
use alife_core::sense::{CHANNELS, CHANNEL_NAMES, N_SCALARS, SCALAR_NAMES};
use alife_core::Sim;
use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2};

use crate::colors::lineage_color;

pub fn show(ui: &mut egui::Ui, sim: &Sim, slot: usize) {
    let Some(ins) = sim.inspect(slot) else {
        ui.label("особь погибла");
        return;
    };

    let id = sim.agents.id[slot];
    let lineage = sim.agents.lineage[slot];
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, lineage_color(lineage));
        ui.heading(format!("особь {id}"));
    });

    egui::Grid::new("agent_facts")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.label(k);
                ui.monospace(v);
                ui.end_row();
            };
            row("слот", format!("{slot}"));
            row("линия", format!("{lineage}"));
            row("родитель", format!("{}", sim.agents.parent[slot]));
            row("поколение", format!("{}", sim.agents.generation[slot]));
            row("возраст", format!("{}", sim.agents.age[slot]));
            row(
                "энергия",
                format!(
                    "{:.2} / {:.0}",
                    sim.agents.energy[slot], sim.cfg.agent.max_energy
                ),
            );
            row(
                "положение",
                format!("{}, {}", sim.agents.x[slot], sim.agents.y[slot]),
            );
        });

    ui.separator();
    ui.label("решение на этот шаг");
    let a = ins.action;
    let dir = ["север", "юг", "запад", "восток"][a.facing as usize % 4];
    ui.monospace(format!(
        "смотрит на {dir}{}{}{}",
        if a.attack {
            ", бьёт"
        } else if a.mv {
            ", идёт"
        } else {
            ", стоит"
        },
        if a.eat { ", ест" } else { "" },
        if a.repro { ", размножается" } else { "" },
    ));

    ui.add_space(4.0);
    ui.label("выходы сети");
    for (i, name) in OUTPUT_NAMES.iter().enumerate() {
        let v = ins.outputs[i];
        // Направление выбирается максимумом из четырёх, остальные три — пороги по нулю.
        let active = match i {
            0 => !a.mv && !a.attack,
            1..=4 => i as u8 - 1 == a.facing,
            5 => a.eat,
            6 => a.attack,
            _ => a.repro,
        };
        signed_bar(ui, name, v, active);
    }

    ui.separator();
    ui.label(format!(
        "окно обзора {side}x{side}, вход в сеть — {n} чисел",
        side = sim.sensor.side,
        n = ins.inputs.len()
    ));
    ui.horizontal_wrapped(|ui| {
        for (c, name) in CHANNEL_NAMES.iter().enumerate() {
            channel_grid(ui, name, &ins.inputs, sim.sensor.side, c);
        }
    });

    if sim.brain.memory > 0 {
        ui.add_space(4.0);
        ui.label(format!("память между шагами ({} чисел)", sim.brain.memory));
        memory_strip(ui, sim.agents.memory(slot));
        ui.small(
            "Выходы последнего скрытого слоя с прошлого шага, возвращённые на вход. \
             Они лежат в пределах ±1. У новорождённой особи память пустая: \
             наследуется сеть, а не состояние.",
        );
    }

    ui.add_space(4.0);
    let scalars_at = sim.sensor.side * sim.sensor.side * CHANNELS;
    egui::Grid::new("scalars")
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for i in 0..N_SCALARS {
                ui.label(SCALAR_NAMES[i]);
                ui.monospace(format!("{:.3}", ins.inputs[scalars_at + i]));
                ui.end_row();
            }
        });
}

/// Столбик со знаком: вправо от середины — положительное значение.
fn signed_bar(ui: &mut egui::Ui, name: &str, v: f32, active: bool) {
    let width = ui.available_width().min(260.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, ui.visuals().extreme_bg_color);

    let bar = Rect::from_min_max(
        rect.min + Vec2::new(96.0, 2.0),
        rect.max - Vec2::new(2.0, 2.0),
    );
    let mid = bar.center().x;
    painter.line_segment(
        [egui::pos2(mid, bar.min.y), egui::pos2(mid, bar.max.y)],
        Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    let t = (v / 4.0).clamp(-1.0, 1.0);
    let half = bar.width() * 0.5;
    let fill = if active {
        Color32::from_rgb(120, 200, 120)
    } else {
        Color32::from_gray(110)
    };
    let x = mid + t * half;
    painter.rect_filled(
        Rect::from_min_max(
            egui::pos2(mid.min(x), bar.min.y),
            egui::pos2(mid.max(x), bar.max.y),
        ),
        1.0,
        fill,
    );

    let text_color = if active {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    painter.text(
        rect.min + Vec2::new(4.0, 1.0),
        egui::Align2::LEFT_TOP,
        name,
        FontId::proportional(11.0),
        text_color,
    );
    painter.text(
        egui::pos2(bar.max.x - 2.0, rect.min.y + 1.0),
        egui::Align2::RIGHT_TOP,
        format!("{v:+.2}"),
        FontId::monospace(10.0),
        text_color,
    );
}

/// Один канал окна обзора как маленькая тепловая карта.
fn channel_grid(ui: &mut egui::Ui, name: &str, inputs: &[f32], side: usize, channel: usize) {
    let cell = 10.0;
    ui.vertical(|ui| {
        ui.small(name);
        let size = Vec2::splat(side as f32 * cell);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter();
        for iy in 0..side {
            for ix in 0..side {
                let v = inputs[(iy * side + ix) * CHANNELS + channel].clamp(0.0, 1.0);
                let g = (20.0 + v * 220.0) as u8;
                let c = Color32::from_rgb(g / 3, g, g / 2);
                let p = rect.min + Vec2::new(ix as f32 * cell, iy as f32 * cell);
                painter.rect_filled(Rect::from_min_size(p, Vec2::splat(cell - 1.0)), 0.0, c);
            }
        }
        // Центр окна — сама особь.
        let mid = side / 2;
        let p = rect.min + Vec2::new(mid as f32 * cell, mid as f32 * cell);
        painter.rect_stroke(
            Rect::from_min_size(p, Vec2::splat(cell - 1.0)),
            0.0,
            Stroke::new(1.0, Color32::from_rgb(240, 200, 80)),
            egui::StrokeKind::Inside,
        );
    });
}

/// Память одной особи полоской: вверх от середины — плюс, вниз — минус.
fn memory_strip(ui: &mut egui::Ui, memory: &[f32]) {
    if memory.is_empty() {
        return;
    }
    let width = ui.available_width().min(260.0);
    let height = 28.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, ui.visuals().extreme_bg_color);

    let mid = rect.center().y;
    painter.line_segment(
        [egui::pos2(rect.min.x, mid), egui::pos2(rect.max.x, mid)],
        Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    let step = rect.width() / memory.len() as f32;
    let half = (height - 4.0) * 0.5;
    for (i, v) in memory.iter().enumerate() {
        let x = rect.min.x + i as f32 * step;
        let h = v.clamp(-1.0, 1.0) * half;
        let color = if *v >= 0.0 {
            Color32::from_rgb(120, 200, 150)
        } else {
            Color32::from_rgb(220, 140, 120)
        };
        painter.rect_filled(
            Rect::from_min_max(
                egui::pos2(x, mid.min(mid - h)),
                egui::pos2(x + (step - 1.0).max(1.0), mid.max(mid - h)),
            ),
            0.0,
            color,
        );
    }
}
