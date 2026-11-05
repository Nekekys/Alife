//! Шаг симуляции.
//!
//! Порядок внутри шага зафиксирован, и это требование, а не деталь:
//! решения считаются параллельно по неизменяемому миру, а применяются
//! строго по возрастанию номера слота. Поэтому один сид даёт один прогон.

use rayon::prelude::*;

use crate::agents::Agents;
use crate::brain::{BrainSpec, Scratch, N_OUTPUTS};
use crate::config::Config;
use crate::metrics::{Counters, Sample};
use crate::rng::Rng;
use crate::sense::{sense, Scales, SensorSpec};
use crate::world::{World, NO_AGENT};

/// Решение особи на один шаг.
#[derive(Debug, Clone, Copy, Default)]
pub struct Action {
    /// Направление взгляда: туда же идёт движение и туда же бьёт атака.
    pub facing: u8,
    pub mv: bool,
    pub eat: bool,
    pub attack: bool,
    pub repro: bool,
}

/// Снимок «мышления» одной особи: что она видит и что решает.
#[derive(Debug, Clone)]
pub struct Inspection {
    pub inputs: Vec<f32>,
    pub outputs: Vec<f32>,
    pub action: Action,
}

#[derive(Debug)]
pub struct Sim {
    pub cfg: Config,
    pub world: World,
    pub agents: Agents,
    pub brain: BrainSpec,
    pub sensor: SensorSpec,
    pub scales: Scales,
    pub rng: Rng,
    pub step: u64,
    pub counters: Counters,
    live_idx: Vec<u32>,
    actions: Vec<Action>,
    initial_mass: f64,
}

impl Sim {
    pub fn new(cfg: Config) -> Result<Self, String> {
        cfg.validate()?;

        let sensor = SensorSpec::new(cfg.brain.vision_radius);
        let brain = BrainSpec::new(sensor.n_inputs(), &cfg.brain.hidden, N_OUTPUTS);
        let scales = Scales::from_config(&cfg);
        let mut world = World::new(cfg.world.width, cfg.world.height);
        let mut agents = Agents::new(cfg.world.capacity, brain.genome_len);
        let mut rng = Rng::new(cfg.seed);

        let cells = world.cells();
        let total = cfg.world.mass_per_cell as f64 * cells as f64;
        let in_agents = cfg.world.init_agents as f64 * cfg.agent.birth_energy as f64;
        if in_agents > total {
            return Err("стартовые особи несут больше массы, чем есть во всём мире".into());
        }
        let per_cell = ((total - in_agents) / cells as f64) as f32;
        let plant0 = per_cell * cfg.world.init_plant_frac;
        for i in 0..cells {
            world.plant[i] = plant0;
            world.nutrient[i] = per_cell - plant0;
        }

        let mut genome = vec![0.0f32; brain.genome_len];
        for _ in 0..cfg.world.init_agents {
            let mut spot = None;
            for _ in 0..64 {
                let x = rng.below(world.w as u32) as u16;
                let y = rng.below(world.h as u32) as u16;
                let c = world.idx(x as usize, y as usize);
                if world.occupant[c] == NO_AGENT {
                    spot = Some((x, y, c));
                    break;
                }
            }
            let (x, y, c) = match spot {
                Some(v) => v,
                // Мир плотно заселён: добираем первую свободную клетку подряд.
                None => match (0..cells).find(|&i| world.occupant[i] == NO_AGENT) {
                    Some(i) => ((i % world.w) as u16, (i / world.w) as u16, i),
                    None => break,
                },
            };
            let slot = match agents.alloc() {
                Some(s) => s,
                None => break,
            };
            agents.spawn(slot, x, y, cfg.agent.birth_energy, 0, 0, 0);
            brain.random_genome(&mut rng, &mut genome);
            agents.genome_mut(slot).copy_from_slice(&genome);
            world.occupant[c] = slot as u32;
        }

        let mut sim = Self {
            cfg,
            world,
            agents,
            brain,
            sensor,
            scales,
            rng,
            step: 0,
            counters: Counters::default(),
            live_idx: Vec::new(),
            actions: Vec::new(),
            initial_mass: 0.0,
        };
        sim.initial_mass = sim.total_mass();
        Ok(sim)
    }

    pub fn tick(&mut self) {
        self.collect_live();
        self.decide();
        self.act();
        let res = self.cfg.resources.clone();
        self.world.update(&res);
        self.counters.ticks += 1;
        self.counters.agent_ticks += self.live_idx.len() as u64;
        self.step += 1;
    }

    pub fn population(&self) -> usize {
        self.agents.live()
    }

    pub fn extinct(&self) -> bool {
        self.agents.live() == 0
    }

    /// Масса мира на момент создания: с ней сравнивается текущая.
    pub fn initial_mass(&self) -> f64 {
        self.initial_mass
    }

    pub fn total_mass(&self) -> f64 {
        let mut m = self.world.cell_mass();
        for i in 0..self.agents.capacity {
            if self.agents.alive[i] {
                m += self.agents.energy[i] as f64;
            }
        }
        m
    }

    /// Сколько памяти занимают геномы. Это главная статья расхода при росте ёмкости.
    pub fn genome_bytes(&self) -> usize {
        self.agents.capacity * self.brain.genome_len * std::mem::size_of::<f32>()
    }

    fn collect_live(&mut self) {
        self.live_idx.clear();
        for i in 0..self.agents.capacity {
            if self.agents.alive[i] {
                self.live_idx.push(i as u32);
            }
        }
    }

    /// Параллельная часть шага: мир здесь только читается.
    fn decide(&mut self) {
        let Sim { world, agents, brain, sensor, scales, live_idx, actions, .. } = self;
        let world: &World = world;
        let agents: &Agents = agents;
        let brain: &BrainSpec = brain;
        let sensor: &SensorSpec = sensor;
        let scales: &Scales = scales;
        let live: &[u32] = live_idx;

        actions.clear();
        actions.resize(live.len(), Action::default());

        actions.par_iter_mut().enumerate().for_each_init(
            || (Scratch::new(brain.max_width), vec![0.0f32; brain.n_in]),
            |(scratch, input), (k, act)| {
                let i = live[k] as usize;
                sense(world, agents, i, sensor, scales, input);
                let out = brain.forward(agents.genome(i), input, scratch);
                *act = decode_action(out);
            },
        );
    }

    /// Последовательная часть шага: порядок применения действий — по слотам.
    fn act(&mut self) {
        let Sim { cfg, world, agents, rng, counters, live_idx, actions, .. } = self;

        for &s in live_idx.iter() {
            let i = s as usize;
            agents.last_ate[i] = 0.0;
            agents.last_hit[i] = 0.0;
        }

        for k in 0..live_idx.len() {
            let i = live_idx[k] as usize;
            // Особь могла погибнуть от чужой атаки на этом же шаге.
            if !agents.alive[i] {
                continue;
            }
            let a = actions[k];
            let x = agents.x[i];
            let y = agents.y[i];
            let cell = world.idx(x as usize, y as usize);
            let mut moved = false;

            // Атака и перемещение исключают друг друга: иначе выгодна тактика «ударил и убежал».
            if a.attack {
                let (tx, ty) = world.step_wrap(x, y, a.facing);
                let tcell = world.idx(tx as usize, ty as usize);
                let cost = cfg.agent.attack_cost.min(agents.energy[i]);
                agents.energy[i] -= cost;
                world.nutrient[cell] += cost;
                counters.attacks += 1;
                let occ = world.occupant[tcell];
                if occ != NO_AGENT {
                    let v = occ as usize;
                    let dmg = cfg.agent.attack_damage.min(agents.energy[v]);
                    agents.energy[v] -= dmg;
                    let room = (cfg.agent.max_energy - agents.energy[i]).max(0.0);
                    let (bite, dropped) = split_damage(dmg, cfg.agent.attack_yield, room);
                    agents.energy[i] += bite;
                    world.carrion[tcell] += dropped;
                    if bite > 0.0 {
                        agents.last_ate[i] = 1.0;
                    }
                    agents.last_hit[v] = 1.0;
                    counters.attacks_hit += 1;
                    counters.damage_dealt += dmg as f64;
                    counters.predated_mass += bite as f64;
                    if agents.energy[v] <= cfg.agent.death_energy {
                        kill(world, agents, v);
                        counters.deaths_killed += 1;
                    }
                }
            } else if a.mv {
                let (nx, ny) = world.step_wrap(x, y, a.facing);
                let ncell = world.idx(nx as usize, ny as usize);
                if world.occupant[ncell] == NO_AGENT {
                    world.occupant[cell] = NO_AGENT;
                    world.occupant[ncell] = i as u32;
                    agents.x[i] = nx;
                    agents.y[i] = ny;
                    moved = true;
                    counters.moves += 1;
                }
            }

            let cur = world.idx(agents.x[i] as usize, agents.y[i] as usize);

            if a.eat {
                let room = (cfg.agent.max_energy - agents.energy[i]).max(0.0);
                let mut want = cfg.agent.eat_rate.min(room);
                if want > 0.0 {
                    // Падаль съедается первой: добыча ценнее травы.
                    let from_carrion = want.min(world.carrion[cur]);
                    world.carrion[cur] -= from_carrion;
                    want -= from_carrion;
                    let from_plant = want.min(world.plant[cur]);
                    world.plant[cur] -= from_plant;
                    let got = from_carrion + from_plant;
                    if got > 0.0 {
                        agents.energy[i] += got;
                        agents.last_ate[i] = 1.0;
                        counters.eats += 1;
                        counters.eaten_plant += from_plant as f64;
                        counters.eaten_carrion += from_carrion as f64;
                    }
                }
            }

            if a.repro && agents.energy[i] >= cfg.agent.repro_threshold {
                let cx = agents.x[i];
                let cy = agents.y[i];
                let start = rng.below(4) as u8;
                let mut spot = None;
                for d in 0..4u8 {
                    let dir = (start + d) % 4;
                    let (nx, ny) = world.step_wrap(cx, cy, dir);
                    let nc = world.idx(nx as usize, ny as usize);
                    if world.occupant[nc] == NO_AGENT {
                        spot = Some((nx, ny, nc));
                        break;
                    }
                }
                if let Some((nx, ny, nc)) = spot {
                    match agents.alloc() {
                        Some(child) => {
                            let e = agents.energy[i];
                            let child_e = e * cfg.agent.repro_share;
                            let overhead = e * cfg.agent.repro_overhead;
                            agents.energy[i] = e - child_e - overhead;
                            world.nutrient[cur] += overhead;

                            let pid = agents.id[i];
                            let lin = agents.lineage[i];
                            let gen = agents.generation[i].saturating_add(1);
                            agents.spawn(child, nx, ny, child_e, pid, lin, gen);
                            agents.copy_genome(i, child);

                            let sigma = cfg.agent.mutation_sigma;
                            let clamp = cfg.agent.weight_clamp;
                            let g = agents.genome_mut(child);
                            rng.add_gaussian(&mut *g, sigma);
                            if clamp > 0.0 {
                                for v in g.iter_mut() {
                                    *v = v.clamp(-clamp, clamp);
                                }
                            }
                            world.occupant[nc] = child as u32;
                            counters.births += 1;
                        }
                        // Ёмкость кончилась: отдельный счётчик, чтобы видеть упор в потолок.
                        None => counters.births_blocked += 1,
                    }
                }
            }

            let mut cost = cfg.agent.metabolism + cfg.agent.age_cost_rate * agents.age[i] as f32;
            if moved {
                cost += cfg.agent.move_cost;
            }
            let cost = cost.min(agents.energy[i]);
            agents.energy[i] -= cost;
            world.nutrient[cur] += cost;
            agents.age[i] = agents.age[i].saturating_add(1);

            if agents.energy[i] <= cfg.agent.death_energy {
                kill(world, agents, i);
                counters.deaths_starved += 1;
            }
        }
    }

    /// Снимает замер и обнуляет счётчики интервала.
    pub fn sample(&mut self) -> Sample {
        let (plant, carrion, nutrient) = self.world.pool_totals();
        let mut mass_agents = 0.0f64;
        let mut sum_age = 0.0f64;
        let mut max_gen = 0u32;
        let mut pop = 0u64;
        for i in 0..self.agents.capacity {
            if !self.agents.alive[i] {
                continue;
            }
            pop += 1;
            mass_agents += self.agents.energy[i] as f64;
            sum_age += self.agents.age[i] as f64;
            max_gen = max_gen.max(self.agents.generation[i]);
        }
        let total = plant + carrion + nutrient + mass_agents;
        let c = self.counters;
        let denom = if c.agent_ticks > 0 { c.agent_ticks as f64 } else { 1.0 };

        let sample = Sample {
            step: self.step,
            population: pop,
            mass_total: total,
            mass_agents,
            mass_plant: plant,
            mass_carrion: carrion,
            mass_nutrient: nutrient,
            agent_mass_frac: if total > 0.0 { mass_agents / total } else { 0.0 },
            mean_energy: if pop > 0 { mass_agents / pop as f64 } else { 0.0 },
            mean_age: if pop > 0 { sum_age / pop as f64 } else { 0.0 },
            max_generation: max_gen,
            births: c.births,
            deaths_starved: c.deaths_starved,
            deaths_killed: c.deaths_killed,
            attacks: c.attacks,
            attacks_hit: c.attacks_hit,
            kills_per_attack: ratio(c.deaths_killed as f64, c.attacks as f64),
            hits_per_attack: ratio(c.attacks_hit as f64, c.attacks as f64),
            kills_per_hit: ratio(c.deaths_killed as f64, c.attacks_hit as f64),
            carrion_share: ratio(c.eaten_carrion, c.eaten_plant + c.eaten_carrion),
            animal_share: ratio(
                c.eaten_carrion + c.predated_mass,
                c.eaten_plant + c.eaten_carrion + c.predated_mass,
            ),
            moves_per_agent: c.moves as f64 / denom,
            eats_per_agent: c.eats as f64 / denom,
            attacks_per_agent: c.attacks as f64 / denom,
            eaten_per_agent: (c.eaten_plant + c.eaten_carrion) / denom,
            births_blocked: c.births_blocked,
        };
        self.counters.reset();
        sample
    }

    /// Кто стоит в клетке. Нужно просмотрщику, чтобы выбрать особь по щелчку.
    pub fn agent_at(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.world.w || y >= self.world.h {
            return None;
        }
        match self.world.occupant[self.world.idx(x, y)] {
            NO_AGENT => None,
            slot => Some(slot as usize),
        }
    }

    /// Пересчитывает входы и выходы сети для одной особи.
    ///
    /// Это ровно то же, что делает параллельная фаза шага, но для одного слота
    /// и без побочных эффектов: тихий баг в сенсорах иначе не отличить
    /// от «эволюция просто идёт медленно».
    pub fn inspect(&self, slot: usize) -> Option<Inspection> {
        if slot >= self.agents.capacity || !self.agents.alive[slot] {
            return None;
        }
        let mut inputs = vec![0.0f32; self.brain.n_in];
        sense(&self.world, &self.agents, slot, &self.sensor, &self.scales, &mut inputs);
        let mut scratch = Scratch::new(self.brain.max_width);
        let outputs = self
            .brain
            .forward(self.agents.genome(slot), &inputs, &mut scratch)
            .to_vec();
        let action = decode_action(&outputs);
        Some(Inspection { inputs, outputs, action })
    }

    /// Отпечаток полного состояния: им проверяется воспроизводимость прогонов.
    pub fn state_hash(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h = fnv(h, self.step);
        h = fnv(h, self.agents.live() as u64);
        for i in 0..self.agents.capacity {
            if !self.agents.alive[i] {
                continue;
            }
            h = fnv(h, i as u64);
            h = fnv(h, self.agents.id[i]);
            h = fnv(h, self.agents.x[i] as u64);
            h = fnv(h, self.agents.y[i] as u64);
            h = fnv(h, self.agents.energy[i].to_bits() as u64);
            h = fnv(h, self.agents.age[i] as u64);
        }
        for i in 0..self.world.cells() {
            h = fnv(h, self.world.plant[i].to_bits() as u64);
            h = fnv(h, self.world.carrion[i].to_bits() as u64);
            h = fnv(h, self.world.nutrient[i].to_bits() as u64);
        }
        h
    }
}

fn kill(world: &mut World, agents: &mut Agents, i: usize) {
    let c = world.idx(agents.x[i] as usize, agents.y[i] as usize);
    debug_assert_eq!(world.occupant[c], i as u32);
    world.carrion[c] += agents.energy[i];
    world.occupant[c] = NO_AGENT;
    agents.release(i);
}

/// Выходы сети: [стоять, север, юг, запад, восток, есть, атаковать, размножиться].
/// Направление — максимум из четырёх направленных выходов; движение происходит,
/// если этот максимум выше выхода «стоять».
fn decode_action(out: &[f32]) -> Action {
    debug_assert_eq!(out.len(), N_OUTPUTS);
    let mut facing = 0u8;
    let mut best = out[1];
    for d in 1..4usize {
        if out[1 + d] > best {
            best = out[1 + d];
            facing = d as u8;
        }
    }
    Action {
        facing,
        mv: best > out[0],
        eat: out[5] > 0.0,
        attack: out[6] > 0.0,
        repro: out[7] > 0.0,
    }
}

/// Делит нанесённый урон на укус атакующему и падаль в клетку жертвы.
///
/// Укус ограничен тем, сколько энергии атакующий ещё вмещает: сытому не
/// достаётся ничего, и весь урон уходит падалью. Сумма всегда равна урону —
/// на этом держится сохранение массы.
fn split_damage(damage: f32, bite_share: f32, attacker_room: f32) -> (f32, f32) {
    let bite = (damage * bite_share).clamp(0.0, attacker_room.max(0.0));
    (bite, damage - bite)
}

/// Отношение, равное нулю при пустом знаменателе: метрика без событий — это 0, а не NaN.
#[inline]
fn ratio(a: f64, b: f64) -> f64 {
    if b > 0.0 {
        a / b
    } else {
        0.0
    }
}

#[inline]
fn fnv(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x0000_0100_0000_01b3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_cfg() -> Config {
        let mut cfg = Config::default();
        cfg.world.width = 48;
        cfg.world.height = 48;
        cfg.world.capacity = 512;
        cfg.world.init_agents = 120;
        cfg.brain.hidden = vec![8];
        cfg
    }

    #[test]
    fn mass_is_conserved_over_a_run() {
        let mut sim = Sim::new(small_cfg()).unwrap();
        let before = sim.initial_mass();
        for _ in 0..500 {
            sim.tick();
        }
        let after = sim.total_mass();
        assert!(
            (after - before).abs() / before < 1e-3,
            "масса уплыла: было {before}, стало {after}"
        );
    }

    #[test]
    fn mass_is_conserved_when_attacks_feed_the_attacker() {
        // Укус перекладывает массу из жертвы в атакующего, а остаток — в падаль.
        // Если атакующий сыт, весь урон обязан уйти в падаль, а не испариться.
        for yield_ in [0.5f32, 1.0] {
            let mut cfg = small_cfg();
            cfg.agent.attack_yield = yield_;
            cfg.agent.attack_damage = 60.0;
            cfg.agent.attack_cost = 0.2;
            let mut sim = Sim::new(cfg).unwrap();
            let before = sim.initial_mass();
            for _ in 0..500 {
                sim.tick();
            }
            let after = sim.total_mass();
            assert!(
                (after - before).abs() / before < 1e-3,
                "укус {yield_}: было {before}, стало {after}"
            );
        }
    }

    #[test]
    fn damage_splits_without_losing_mass() {
        // Сытому укус ничего не даёт: весь урон обязан уйти падалью.
        assert_eq!(split_damage(40.0, 1.0, 0.0), (0.0, 40.0));
        // Голодному с запасом — ровно оговорённая доля.
        assert_eq!(split_damage(40.0, 0.75, 100.0), (30.0, 10.0));
        // Места меньше, чем укус: лишнее падает падалью.
        assert_eq!(split_damage(40.0, 1.0, 12.0), (12.0, 28.0));
        // Атака не кормит вовсе.
        assert_eq!(split_damage(40.0, 0.0, 100.0), (0.0, 40.0));

        for &(d, y, room) in &[(40.0f32, 0.3f32, 5.0f32), (7.5, 1.0, 3.25), (0.0, 0.5, 10.0)] {
            let (bite, dropped) = split_damage(d, y, room);
            assert!(bite >= 0.0 && dropped >= 0.0, "отрицательная доля при {d}/{y}/{room}");
            assert!(bite <= room, "укус больше, чем вмещает атакующий");
            assert!((bite + dropped - d).abs() < 1e-6, "сумма долей не равна урону");
        }
    }

    #[test]
    fn same_seed_gives_same_run() {
        let mut a = Sim::new(small_cfg()).unwrap();
        let mut b = Sim::new(small_cfg()).unwrap();
        for _ in 0..300 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.population(), b.population());
    }

    #[test]
    fn different_seeds_diverge() {
        let mut cfg_b = small_cfg();
        cfg_b.seed = 999;
        let mut a = Sim::new(small_cfg()).unwrap();
        let mut b = Sim::new(cfg_b).unwrap();
        for _ in 0..100 {
            a.tick();
            b.tick();
        }
        assert_ne!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn inspection_predicts_the_next_move() {
        // Одна особь в пустом мире: ей никто не мешает, поэтому предсказанное
        // инспектором перемещение должно совпасть с фактическим.
        let mut cfg = small_cfg();
        cfg.world.init_agents = 1;
        let mut sim = Sim::new(cfg).unwrap();
        let slot = (0..sim.agents.capacity).find(|&i| sim.agents.alive[i]).unwrap();

        let before = (sim.agents.x[slot], sim.agents.y[slot]);
        let ins = sim.inspect(slot).unwrap();
        assert_eq!(ins.inputs.len(), sim.brain.n_in);
        assert_eq!(ins.outputs.len(), N_OUTPUTS);

        let expected = if ins.action.attack || !ins.action.mv {
            before
        } else {
            sim.world.step_wrap(before.0, before.1, ins.action.facing)
        };
        sim.tick();
        assert_eq!((sim.agents.x[slot], sim.agents.y[slot]), expected);
    }

    #[test]
    fn inspection_of_an_empty_slot_is_none() {
        let sim = Sim::new(small_cfg()).unwrap();
        let empty = (0..sim.agents.capacity).find(|&i| !sim.agents.alive[i]).unwrap();
        assert!(sim.inspect(empty).is_none());
        assert!(sim.inspect(sim.agents.capacity).is_none());
    }

    #[test]
    fn occupancy_matches_agent_positions() {
        let mut sim = Sim::new(small_cfg()).unwrap();
        for _ in 0..200 {
            sim.tick();
        }
        let mut seen = 0;
        for i in 0..sim.agents.capacity {
            if !sim.agents.alive[i] {
                continue;
            }
            let c = sim.world.idx(sim.agents.x[i] as usize, sim.agents.y[i] as usize);
            assert_eq!(sim.world.occupant[c], i as u32, "слот {i} не на своём месте");
            seen += 1;
        }
        let marked = sim.world.occupant.iter().filter(|&&o| o != NO_AGENT).count();
        assert_eq!(seen, marked, "в сетке отмечено клеток больше, чем живых особей");
    }
}
