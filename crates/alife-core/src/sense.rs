//! Сенсоры особи: окно обзора плюс несколько скалярных входов.
//!
//! Все входы нормируются в 0..1 — это первое место, где легко завести
//! тихий баг, поэтому масштабы считаются один раз из конфига и покрыты тестами.

use crate::agents::Agents;
use crate::config::Config;
use crate::world::{World, NO_AGENT};

/// Каналы окна обзора: растения, падаль, питательные вещества, энергия соседа.
pub const CHANNELS: usize = 4;
/// Скалярные входы: энергия, возраст, ела, били, плотность, растения и падаль под собой, смещение.
pub const N_SCALARS: usize = 8;
/// Возраст, при котором соответствующий вход насыщается.
const AGE_SCALE: f32 = 1000.0;

#[derive(Debug, Clone, Copy)]
pub struct SensorSpec {
    pub radius: usize,
    pub side: usize,
}

impl SensorSpec {
    pub fn new(radius: usize) -> Self {
        Self { radius, side: 2 * radius + 1 }
    }

    pub fn n_inputs(&self) -> usize {
        self.side * self.side * CHANNELS + N_SCALARS
    }
}

/// Обратные величины масштабов: деление на горячем пути заменено умножением.
#[derive(Debug, Clone, Copy)]
pub struct Scales {
    pub inv_plant: f32,
    pub inv_carrion: f32,
    pub inv_nutrient: f32,
    pub inv_energy: f32,
    pub inv_age: f32,
}

impl Scales {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            inv_plant: 1.0 / cfg.resources.plant_capacity.max(1e-6),
            inv_carrion: 1.0 / cfg.agent.birth_energy.max(1e-6),
            inv_nutrient: 1.0 / cfg.world.mass_per_cell.max(1e-6),
            inv_energy: 1.0 / cfg.agent.max_energy.max(1e-6),
            inv_age: 1.0 / AGE_SCALE,
        }
    }
}

/// Заполняет вектор входов сети для особи `slot`.
pub fn sense(
    world: &World,
    agents: &Agents,
    slot: usize,
    spec: &SensorSpec,
    scales: &Scales,
    out: &mut [f32],
) {
    debug_assert_eq!(out.len(), spec.n_inputs());
    let ax = agents.x[slot] as i32;
    let ay = agents.y[slot] as i32;
    let r = spec.radius as i32;

    let mut k = 0usize;
    let mut occupied = 0u32;
    for dy in -r..=r {
        for dx in -r..=r {
            let c = world.idx_wrap(ax + dx, ay + dy);
            out[k] = (world.plant[c] * scales.inv_plant).min(1.0);
            out[k + 1] = (world.carrion[c] * scales.inv_carrion).min(1.0);
            out[k + 2] = (world.nutrient[c] * scales.inv_nutrient).min(1.0);
            let occ = world.occupant[c];
            out[k + 3] = if occ == NO_AGENT {
                0.0
            } else {
                occupied += 1;
                (agents.energy[occ as usize] * scales.inv_energy).min(1.0)
            };
            k += CHANNELS;
        }
    }

    let here = world.idx(agents.x[slot] as usize, agents.y[slot] as usize);
    let window = (spec.side * spec.side) as f32;
    out[k] = (agents.energy[slot] * scales.inv_energy).min(1.0);
    out[k + 1] = (agents.age[slot] as f32 * scales.inv_age).min(1.0);
    out[k + 2] = agents.last_ate[slot];
    out[k + 3] = agents.last_hit[slot];
    out[k + 4] = occupied as f32 / window;
    out[k + 5] = (world.plant[here] * scales.inv_plant).min(1.0);
    out[k + 6] = (world.carrion[here] * scales.inv_carrion).min(1.0);
    out[k + 7] = 1.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::BrainSpec;

    fn setup() -> (World, Agents, Config) {
        let cfg = Config::default();
        let world = World::new(16, 16);
        let spec = SensorSpec::new(cfg.brain.vision_radius);
        let brain = BrainSpec::new(spec.n_inputs(), &cfg.brain.hidden, crate::brain::N_OUTPUTS);
        let agents = Agents::new(8, brain.genome_len);
        (world, agents, cfg)
    }

    #[test]
    fn input_count_matches_brain_input() {
        let spec = SensorSpec::new(3);
        assert_eq!(spec.side, 7);
        assert_eq!(spec.n_inputs(), 7 * 7 * CHANNELS + N_SCALARS);
    }

    #[test]
    fn all_inputs_are_normalised() {
        let (mut world, mut agents, cfg) = setup();
        let spec = SensorSpec::new(cfg.brain.vision_radius);
        let scales = Scales::from_config(&cfg);
        // Заполняем мир значениями заведомо выше любых масштабов.
        for i in 0..world.cells() {
            world.plant[i] = 1e6;
            world.carrion[i] = 1e6;
            world.nutrient[i] = 1e6;
        }
        let slot = agents.alloc().unwrap();
        agents.spawn(slot, 2, 2, 1e6, 0, 0, 0);
        agents.age[slot] = 1_000_000;
        let here = world.idx(2, 2);
        world.occupant[here] = slot as u32;

        let mut out = vec![0.0; spec.n_inputs()];
        sense(&world, &agents, slot, &spec, &scales, &mut out);
        assert!(
            out.iter().all(|v| (0.0..=1.0).contains(v)),
            "вход вне диапазона 0..1"
        );
    }

    #[test]
    fn empty_cells_read_as_zero() {
        let (world, mut agents, cfg) = setup();
        let spec = SensorSpec::new(cfg.brain.vision_radius);
        let scales = Scales::from_config(&cfg);
        let slot = agents.alloc().unwrap();
        agents.spawn(slot, 8, 8, 0.0, 0, 0, 0);
        let mut out = vec![0.0; spec.n_inputs()];
        sense(&world, &agents, slot, &spec, &scales, &mut out);
        let window = spec.side * spec.side * CHANNELS;
        assert!(out[..window].iter().all(|v| *v == 0.0));
        // Особь не отмечена в occupant, поэтому плотность нулевая.
        assert_eq!(out[window + 4], 0.0);
        assert_eq!(out[spec.n_inputs() - 1], 1.0);
    }
}
