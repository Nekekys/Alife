//! Вкладка «разнообразие»: родословная, генетические кластеры и рацион.

use alife_core::{
    best_lag, detrend, lagged_correlation, oscillation_period, Genetics, Sample, Sim,
};
use eframe::egui::{self, Color32};

use crate::charts::line_chart;

/// Пороги склейки кластеров, в мутационных шагах. Число кластеров всегда
/// зависит от порога, поэтому показываются сразу несколько.
pub const THRESHOLDS: [f64; 5] = [2.0, 5.0, 10.0, 20.0, 40.0];

pub struct DiversityState {
    pub scan: Vec<Genetics>,
    pub computed_at: Option<u64>,
    pub sample: usize,
    /// Пересчитывать раз в столько шагов; 0 — только по кнопке.
    pub every: u64,
}

impl Default for DiversityState {
    fn default() -> Self {
        Self { scan: Vec::new(), computed_at: None, sample: 512, every: 2000 }
    }
}

impl DiversityState {
    pub fn recompute(&mut self, sim: &Sim) {
        self.scan = sim.genetics_scan(self.sample, &THRESHOLDS);
        self.computed_at = Some(sim.step);
    }
}

pub fn show(ui: &mut egui::Ui, sim: &Sim, state: &mut DiversityState, history: &[Sample]) {
    ui.heading("Разнообразие");

    let ph = sim.phylo_stats();
    ui.label("Родословная");
    egui::Grid::new("phylo").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            ui.label(k);
            ui.monospace(v);
            ui.end_row();
        };
        row("живых", format!("{}", sim.population()));
        row("узлов в дереве", format!("{}", ph.nodes));
        row("выживших линий", format!("{}", ph.roots));
        row("глубже всех поколение", format!("{}", ph.max_generation));
        if ph.roots == 1 {
            row(
                "общий предок",
                format!("поколение {}, шаг {}", ph.mrca_generation, ph.mrca_step),
            );
        } else {
            row("общий предок", "нет: линии не сходятся".into());
        }
    });
    ui.small(
        "Дерево хранит только предков живых. Ветвь без живых потомков удаляется \
         сразу, поэтому размер дерева говорит о том, насколько переплетена родня, \
         а не о том, сколько особей жило всего.",
    );

    ui.separator();
    ui.label("Рацион");
    let diet = sim.diet_split(sim.cfg.agent.birth_energy, 0.5);
    egui::Grid::new("diet").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
        let mut row = |k: &str, v: String| {
            ui.label(k);
            ui.monospace(v);
            ui.end_row();
        };
        row("хищных", format!("{}", diet.predators));
        row("травоядных", format!("{}", diet.herbivores));
        row("ещё не судить", format!("{}", diet.undecided));
    });
    ui.small(
        "Это не виды и не родственные группы: особь попадает в ту или другую \
         половину по тому, что съела за жизнь. Пока съедено меньше собственного \
         веса, судить не о чем.",
    );

    ui.add_space(4.0);
    lotka_volterra(ui, history);

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Генетика");
        if ui.button("пересчитать").clicked() {
            state.recompute(sim);
        }
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::Slider::new(&mut state.sample, 64..=2048)
                .logarithmic(true)
                .text("особей в выборке"),
        );
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::Slider::new(&mut state.every, 0..=20000)
                .step_by(500.0)
                .text("пересчёт раз в N шагов"),
        );
    });
    ui.small("Проекция геномов на порядок дороже остальных метрик, поэтому считается редко.");

    ui.add_space(4.0);
    match (state.computed_at, state.scan.first()) {
        (Some(at), Some(g)) => {
            ui.label(format!("замер на шаге {at}, выборка {} особей", g.sampled));
            ui.monospace(format!(
                "расстояние между геномами: {:.1} мутационных шагов\n\
                 медиана {:.1}, от {:.1} до {:.1}",
                g.mean_distance, g.median_distance, g.p10_distance, g.p90_distance
            ));
            ui.small(
                "Мутационный шаг — это расстояние, которое геном проходит за одно \
                 рождение. В нём меряются все расстояния, иначе числа зависели бы \
                 от длины генома.",
            );

            ui.add_space(4.0);
            ui.label("Кластеры при разных порогах склейки");
            egui::Grid::new("clusters").num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
                ui.strong("порог");
                ui.strong("кластеров");
                ui.strong("в крупнейшем");
                ui.end_row();
                for g in &state.scan {
                    ui.monospace(format!("{:.0}", g.threshold));
                    ui.monospace(format!("{}", g.clusters));
                    ui.monospace(format!("{:.0}%", g.largest_cluster_share * 100.0));
                    ui.end_row();
                }
            });
            ui.small(
                "Если популяция распалась на виды, при каком-то пороге должно \
                 появиться несколько сопоставимых по размеру кластеров. Плавный \
                 переход от множества мелких к одному большому означает, что \
                 видов нет: это одно сплошное облако.",
            );
        }
        _ => {
            ui.label("генетика ещё не считалась");
        }
    }

    if history.len() > 2 {
        ui.separator();
        let xs: Vec<f64> = history.iter().map(|s| s.step as f64).collect();
        let lineages: Vec<f64> = history.iter().map(|s| s.lineages as f64).collect();
        let predators: Vec<f64> = history.iter().map(|s| s.predators as f64).collect();
        let herbivores: Vec<f64> = history.iter().map(|s| s.herbivores as f64).collect();
        let nodes: Vec<f64> = history.iter().map(|s| s.phylo_nodes as f64).collect();
        line_chart(ui, "выживших линий", &xs, &lineages, Color32::from_rgb(190, 170, 255), 70.0);
        line_chart(ui, "узлов в дереве", &xs, &nodes, Color32::from_rgb(150, 190, 220), 70.0);
        line_chart(ui, "хищных", &xs, &predators, Color32::from_rgb(255, 150, 120), 70.0);
        line_chart(ui, "травоядных", &xs, &herbivores, Color32::from_rgb(150, 230, 150), 70.0);
    }
}

fn lotka_volterra(ui: &mut egui::Ui, history: &[Sample]) {
    ui.label("Цикл хищник — жертва");
    if history.len() < 20 {
        ui.small("нужно больше замеров");
        return;
    }
    let herb: Vec<f64> = history.iter().map(|s| s.herbivores as f64).collect();
    let pred: Vec<f64> = history.iter().map(|s| s.predators as f64).collect();
    let step = history
        .windows(2)
        .next()
        .map(|w| w[1].step.saturating_sub(w[0].step))
        .unwrap_or(1)
        .max(1);
    // Пока охота разворачивается, численность хищных растёт весь прогон.
    // Без вычитания тренда корреляция меряет этот рост, а не цикл.
    let window = (history.len() / 8).max(5) | 1;
    let flat_h = detrend(&herb, window);
    let flat_p = detrend(&pred, window);
    let max_lag = history.len() / 8;
    let period = oscillation_period(&flat_p, history.len() / 3);

    match period {
        Some((lag, corr)) => ui.monospace(format!(
            "колебания хищных: период около {} шагов (автокорреляция возвращается к {corr:.2})",
            lag as u64 * step
        )),
        None => ui.monospace("колебаний численности хищных не видно".to_owned()),
    };

    // Хищные и травоядные делят одну популяцию, поэтому их доли могут
    // оказаться почти зеркальными. У зеркальных рядов пик кросс-корреляции
    // всегда приходится на полпериода, что бы ни происходило на самом деле.
    let pop: Vec<f64> = history.iter().map(|s| s.population.max(1) as f64).collect();
    let share = |xs: &[f64]| -> Vec<f64> { xs.iter().zip(&pop).map(|(x, p)| x / p).collect() };
    let mirror = lagged_correlation(
        &detrend(&share(&herb), window),
        &detrend(&share(&pred), window),
        0,
    );
    ui.monospace(format!("зеркальность долей: {mirror:.2}"));

    let (lag, corr) = best_lag(&flat_h, &flat_p, max_lag);
    ui.monospace(format!(
        "жертвы против хищников: задержка {} шагов, корреляция {corr:.3}",
        lag as u64 * step
    ));
    if lag == 0 || lag == max_lag {
        ui.small("задержка на краю окна поиска: скорее всего это не цикл");
    } else if period.is_some_and(|(p, _)| {
        (lag as f64 - p as f64 / 2.0).abs() <= (p as f64 * 0.2).max(1.0)
    }) {
        ui.small("задержка равна полупериоду — это следствие зеркальности, а не причинности");
    } else if mirror < -0.8 {
        ui.small("доли почти зеркальны: о том, кто за кем следует, судить нельзя");
    }
    ui.small(
        "В цикле Лотки–Вольтерры численность хищников повторяет численность жертв \
         с задержкой. Но пока хищник и жертва — одна популяция, их доли в сумме \
         дают единицу и потому зеркальны, а у зеркальных рядов задержка всегда \
         выходит в полпериода независимо от того, кто на кого влияет.",
    );
}
