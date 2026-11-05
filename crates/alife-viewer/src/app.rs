//! Состояние окна и компоновка панелей.

use std::path::PathBuf;
use std::time::Instant;

use alife_core::{Config, Sample, Sim};
use eframe::egui::{self, Color32};

use crate::charts::line_chart;
use crate::inspector;
use crate::worldview::{ColorMode, WorldView};

/// Сколько замеров держим для графиков.
const HISTORY: usize = 1200;

#[derive(Debug, Clone, Copy)]
struct Selected {
    slot: usize,
    id: u64,
}

pub struct ViewerApp {
    sim: Sim,
    base_cfg: Config,
    cfg_path: Option<PathBuf>,
    seed: u64,
    running: bool,
    steps_per_frame: usize,
    sample_every: u64,
    history: Vec<Sample>,
    view: WorldView,
    selected: Option<Selected>,
    follow: bool,
    steps_per_sec: f64,
    status: String,
}

impl ViewerApp {
    pub fn new(cfg: Config, cfg_path: Option<PathBuf>) -> Result<Self, String> {
        let seed = cfg.seed;
        let sim = Sim::new(cfg.clone())?;
        Ok(Self {
            sim,
            base_cfg: cfg,
            cfg_path,
            seed,
            running: false,
            steps_per_frame: 4,
            sample_every: 25,
            history: Vec::with_capacity(HISTORY),
            view: WorldView::default(),
            selected: None,
            follow: false,
            steps_per_sec: 0.0,
            status: String::new(),
        })
    }

    fn restart(&mut self) {
        let mut cfg = self.base_cfg.clone();
        cfg.seed = self.seed;
        match Sim::new(cfg) {
            Ok(sim) => {
                self.sim = sim;
                self.history.clear();
                self.selected = None;
                self.view.request_fit();
                self.status = format!("новый прогон, сид {}", self.seed);
            }
            Err(e) => self.status = format!("конфиг не принят: {e}"),
        }
    }

    fn reload_config(&mut self) {
        let Some(path) = self.cfg_path.clone() else {
            self.status = "файл конфигурации не задан".into();
            return;
        };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| {
            toml::from_str::<Config>(&t).map_err(|e| e.to_string())
        }) {
            Ok(cfg) => {
                self.base_cfg = cfg;
                self.restart();
                self.status = format!("конфиг перечитан: {}", path.display());
            }
            Err(e) => self.status = format!("{}: {e}", path.display()),
        }
    }

    fn advance(&mut self, steps: usize) {
        let started = Instant::now();
        let mut done = 0usize;
        for _ in 0..steps {
            self.sim.tick();
            done += 1;
            if self.sim.step % self.sample_every == 0 {
                let s = self.sim.sample();
                if self.history.len() == HISTORY {
                    self.history.remove(0);
                }
                self.history.push(s);
            }
            if self.sim.extinct() {
                self.running = false;
                self.status = format!("популяция вымерла на шаге {}", self.sim.step);
                break;
            }
        }
        let dt = started.elapsed().as_secs_f64();
        if dt > 0.0 && done > 0 {
            let now = done as f64 / dt;
            self.steps_per_sec = if self.steps_per_sec == 0.0 {
                now
            } else {
                self.steps_per_sec * 0.85 + now * 0.15
            };
        }
    }

    /// Выбранная особь ещё жива и не подменена на другую в том же слоте?
    fn valid_selection(&self) -> Option<usize> {
        let s = self.selected?;
        (self.sim.agents.alive[s.slot] && self.sim.agents.id[s.slot] == s.id).then_some(s.slot)
    }

    fn select_random(&mut self) {
        let live: Vec<usize> = (0..self.sim.agents.capacity)
            .filter(|&i| self.sim.agents.alive[i])
            .collect();
        if live.is_empty() {
            self.status = "живых особей нет".into();
            return;
        }
        // Берём середину списка: не нужен генератор, а слот каждый раз другой.
        let slot = live[(self.sim.step as usize).wrapping_mul(2654435761) % live.len()];
        self.selected = Some(Selected { slot, id: self.sim.agents.id[slot] });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("alife");
        ui.horizontal(|ui| {
            if ui.button(if self.running { "пауза" } else { "пуск" }).clicked() {
                self.running = !self.running;
            }
            if ui.button("шаг").clicked() {
                self.running = false;
                self.advance(1);
            }
            if ui.button("сброс").clicked() {
                self.restart();
            }
        });

        ui.add(
            egui::Slider::new(&mut self.steps_per_frame, 1..=256)
                .logarithmic(true)
                .text("шагов за кадр"),
        );
        ui.add(
            egui::Slider::new(&mut self.sample_every, 1..=500)
                .logarithmic(true)
                .text("замер раз в N шагов"),
        );

        ui.horizontal(|ui| {
            ui.label("сид");
            ui.add(egui::DragValue::new(&mut self.seed).speed(1.0));
            if ui.button("заново с этим сидом").clicked() {
                self.restart();
            }
        });
        if self.cfg_path.is_some() && ui.button("перечитать конфиг").clicked() {
            self.reload_config();
        }

        ui.separator();
        egui::Grid::new("stats").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
            let mut row = |k: &str, v: String| {
                ui.label(k);
                ui.monospace(v);
                ui.end_row();
            };
            row("шаг", format!("{}", self.sim.step));
            row("особей", format!("{}", self.sim.population()));
            row("ёмкость", format!("{}", self.sim.agents.capacity));
            row("шагов/с", format!("{:.0}", self.steps_per_sec));
            if let Some(s) = self.history.last() {
                row("в особях", format!("{:.1}%", s.agent_mass_frac * 100.0));
                row("поколение", format!("{}", s.max_generation));
                row("убийств/атаку", format!("{:.4}", s.kills_per_attack));
            }
            let drift =
                (self.sim.total_mass() - self.sim.initial_mass()).abs() / self.sim.initial_mass();
            row("уход массы", format!("{drift:.1e}"));
        });

        ui.separator();
        ui.label("раскраска особей");
        ui.horizontal_wrapped(|ui| {
            for (mode, name) in ColorMode::ALL {
                ui.radio_value(&mut self.view.mode, mode, name);
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.follow, "следить за выбранной");
            if ui.button("вписать мир").clicked() {
                self.view.request_fit();
            }
        });
        if ui.button("случайная особь").clicked() {
            self.select_random();
        }

        if !self.status.is_empty() {
            ui.separator();
            ui.colored_label(Color32::from_rgb(220, 180, 90), &self.status);
        }

        ui.separator();
        match self.valid_selection() {
            Some(slot) => inspector::show(ui, &self.sim, slot),
            None => {
                ui.label("щёлкните по особи на карте, чтобы раскрыть её сенсоры и решения");
            }
        }
    }

    fn charts(&mut self, ui: &mut egui::Ui) {
        if self.history.len() < 2 {
            ui.label("графики появятся после первых замеров");
            return;
        }
        let xs: Vec<f64> = self.history.iter().map(|s| s.step as f64).collect();
        let pop: Vec<f64> = self.history.iter().map(|s| s.population as f64).collect();
        let frac: Vec<f64> = self.history.iter().map(|s| s.agent_mass_frac * 100.0).collect();
        let eats: Vec<f64> = self.history.iter().map(|s| s.eats_per_agent).collect();
        let kpa: Vec<f64> = self.history.iter().map(|s| s.kills_per_attack).collect();
        let age: Vec<f64> = self.history.iter().map(|s| s.mean_age).collect();
        let plant: Vec<f64> = self.history.iter().map(|s| s.mass_plant).collect();

        let h = (ui.available_height() - 8.0).max(60.0);
        let row = (h / 2.0 - 4.0).clamp(50.0, 140.0);
        ui.columns(3, |c| {
            line_chart(&mut c[0], "численность", &xs, &pop, Color32::from_rgb(120, 200, 255), row);
            line_chart(&mut c[1], "биомасса в особях, %", &xs, &frac, Color32::from_rgb(150, 230, 150), row);
            line_chart(&mut c[2], "средний возраст", &xs, &age, Color32::from_rgb(230, 200, 120), row);
        });
        ui.add_space(4.0);
        ui.columns(3, |c| {
            line_chart(&mut c[0], "растительная масса", &xs, &plant, Color32::from_rgb(120, 220, 160), row);
            line_chart(&mut c[1], "еда на особь за шаг", &xs, &eats, Color32::from_rgb(200, 170, 255), row);
            line_chart(&mut c[2], "убийств на атаку", &xs, &kpa, Color32::from_rgb(255, 130, 130), row);
        });
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let (space, dot) = ui.input(|i| {
            (i.key_pressed(egui::Key::Space), i.key_pressed(egui::Key::Period))
        });
        if space {
            self.running = !self.running;
        }
        if dot {
            self.running = false;
            self.advance(1);
        }

        if self.running {
            let n = self.steps_per_frame;
            self.advance(n);
        }

        egui::Panel::left("controls")
            .default_size(360.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });

        egui::Panel::bottom("charts")
            .default_size(230.0)
            .resizable(true)
            .show(ui, |ui| self.charts(ui));

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let max_gen = self.history.last().map_or(1, |s| s.max_generation);
            self.view.refresh(ui.ctx(), &self.sim, max_gen);

            if self.follow {
                if let Some(slot) = self.valid_selection() {
                    let rect = ui.available_rect_before_wrap();
                    let (x, y) = (self.sim.agents.x[slot] as f32, self.sim.agents.y[slot] as f32);
                    self.view.center_on(rect, x, y);
                }
            }

            let (_, clicked) = self.view.show(ui, &self.sim);
            if let Some((x, y)) = clicked {
                match self.sim.agent_at(x, y) {
                    Some(slot) => {
                        self.selected = Some(Selected { slot, id: self.sim.agents.id[slot] });
                        self.status.clear();
                    }
                    None => self.status = format!("в клетке {x}, {y} пусто"),
                }
            }
        });

        if self.running {
            ui.ctx().request_repaint();
        }
    }
}
