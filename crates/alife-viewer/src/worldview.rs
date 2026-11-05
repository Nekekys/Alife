//! Карта мира: сборка текстуры из состояния симуляции и камера.

use alife_core::sense::Scales;
use alife_core::world::NO_AGENT;
use alife_core::Sim;
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Response, Sense, TextureHandle, Vec2};

use crate::colors::{cell_color, diet_color, heat, lineage_color};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Lineage,
    Energy,
    Age,
    Generation,
    Diet,
}

impl ColorMode {
    pub const ALL: [(ColorMode, &'static str); 5] = [
        (ColorMode::Lineage, "линии"),
        (ColorMode::Energy, "энергия"),
        (ColorMode::Age, "возраст"),
        (ColorMode::Generation, "поколение"),
        (ColorMode::Diet, "рацион"),
    ];
}

pub struct WorldView {
    texture: Option<TextureHandle>,
    pixels: Vec<Color32>,
    pub mode: ColorMode,
    /// Рисовать ли ресурсы фоном. Без них видно одних существ.
    pub show_resources: bool,
    pub show_agents: bool,
    /// Пикселей на клетку.
    pub scale: f32,
    /// Клетка мира в левом верхнем углу области просмотра.
    pub origin: Vec2,
    fitted: bool,
}

impl Default for WorldView {
    fn default() -> Self {
        Self {
            texture: None,
            pixels: Vec::new(),
            mode: ColorMode::Lineage,
            show_resources: true,
            show_agents: true,
            scale: 3.0,
            origin: Vec2::ZERO,
            fitted: false,
        }
    }
}

impl WorldView {
    /// Пересобирает картинку мира. Сначала ресурсы, поверх — особи.
    pub fn refresh(&mut self, ctx: &egui::Context, sim: &Sim, max_generation: u32) {
        let w = sim.world.w;
        let h = sim.world.h;
        let scales = Scales::from_config(&sim.cfg);
        self.pixels.resize(w * h, Color32::BLACK);

        if self.show_resources {
            for i in 0..w * h {
                self.pixels[i] = cell_color(
                    sim.world.plant[i] * scales.inv_plant,
                    sim.world.carrion[i] * scales.inv_carrion,
                    sim.world.nutrient[i] * scales.inv_nutrient,
                );
            }
        } else {
            // Чистый вид: без фона каждая особь видна сама по себе.
            self.pixels.iter_mut().for_each(|p| *p = Color32::from_gray(6));
        }

        if self.show_agents {
            let max_energy = sim.cfg.agent.max_energy.max(1e-6);
            let gen_span = max_generation.max(1) as f32;
            let min_intake = sim.cfg.agent.birth_energy;
            for i in 0..w * h {
                let occ = sim.world.occupant[i];
                if occ == NO_AGENT {
                    continue;
                }
                let slot = occ as usize;
                self.pixels[i] = match self.mode {
                    ColorMode::Lineage => lineage_color(sim.agents.lineage[slot]),
                    ColorMode::Energy => heat(sim.agents.energy[slot] / max_energy),
                    // 1000 шагов — тот же масштаб, в котором возраст видит сама особь.
                    ColorMode::Age => heat(sim.agents.age[slot] as f32 / 1000.0),
                    ColorMode::Generation => heat(sim.agents.generation[slot] as f32 / gen_span),
                    ColorMode::Diet => diet_color(
                        sim.agents.eaten_plant[slot],
                        sim.agents.eaten_animal[slot],
                        min_intake,
                    ),
                };
            }
        }

        let image = ColorImage::new([w, h], self.pixels.clone());
        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("world", image, egui::TextureOptions::NEAREST))
            }
        }
    }

    /// Рисует карту и возвращает отклик плюс клетку под курсором при щелчке.
    pub fn show(&mut self, ui: &mut egui::Ui, sim: &Sim) -> (Response, Option<(usize, usize)>) {
        let avail = ui.available_size();
        let (response, painter) = ui.allocate_painter(avail, Sense::click_and_drag());
        let rect = response.rect;
        let (w, h) = (sim.world.w as f32, sim.world.h as f32);

        if !self.fitted && rect.width() > 1.0 {
            self.fit(rect, w, h);
            self.fitted = true;
        }

        // Колесо мыши: масштаб меняется вокруг курсора, клетка под ним стоит на месте.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    let before = self.cell_at(p, rect);
                    self.scale = (self.scale * (1.0 + scroll * 0.002)).clamp(0.25, 64.0);
                    let after = self.cell_at(p, rect);
                    self.origin += before - after;
                }
            }
        }
        if response.dragged() {
            self.origin -= response.drag_delta() / self.scale;
        }

        painter.rect_filled(rect, 0.0, Color32::from_gray(8));
        if let Some(tex) = &self.texture {
            let size = Vec2::new(w * self.scale, h * self.scale);
            let min = rect.min - self.origin * self.scale;
            painter.image(
                tex.id(),
                Rect::from_min_size(min, size),
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let clicked = if response.clicked() {
            response.interact_pointer_pos().map(|p| {
                let c = self.cell_at(p, rect);
                // Мир — тор, поэтому щелчок за краем попадает на другую сторону.
                let x = (c.x.floor() as i64).rem_euclid(sim.world.w as i64) as usize;
                let y = (c.y.floor() as i64).rem_euclid(sim.world.h as i64) as usize;
                (x, y)
            })
        } else {
            None
        };

        (response, clicked)
    }

    pub fn fit(&mut self, rect: Rect, w: f32, h: f32) {
        self.scale = (rect.width() / w).min(rect.height() / h).max(0.25);
        self.center_on(rect, w * 0.5, h * 0.5);
    }

    pub fn center_on(&mut self, rect: Rect, cx: f32, cy: f32) {
        self.origin = Vec2::new(
            cx - rect.width() * 0.5 / self.scale,
            cy - rect.height() * 0.5 / self.scale,
        );
    }

    pub fn request_fit(&mut self) {
        self.fitted = false;
    }

    /// Экранная точка в координатах клеток, дробных.
    fn cell_at(&self, p: Pos2, rect: Rect) -> Vec2 {
        self.origin + (p - rect.min) / self.scale
    }
}
