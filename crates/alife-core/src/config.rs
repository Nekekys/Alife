//! Параметры мира. Всё, что задаёт баланс экосистемы, собрано здесь:
//! именно этот набор чисел позже будет подбирать автонастройка.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Сид генератора. Один сид даёт один и тот же прогон.
    pub seed: u64,
    pub world: WorldConfig,
    pub brain: BrainConfig,
    pub agent: AgentConfig,
    pub resources: ResourceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 1,
            world: WorldConfig::default(),
            brain: BrainConfig::default(),
            agent: AgentConfig::default(),
            resources: ResourceConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldConfig {
    pub width: usize,
    pub height: usize,
    /// Верхний предел числа особей: размер массивов, выделяемых один раз.
    pub capacity: usize,
    pub init_agents: usize,
    /// Полная масса мира на клетку. Это число неизменно весь прогон.
    /// Задаёт плотность населения, а она решает, станет ли охота заметной:
    /// при вчетверо меньшем значении хищничество остаётся 1% рациона.
    pub mass_per_cell: f32,
    /// Доля свободной массы, лежащая на старте в растениях; остальное — питательные вещества.
    pub init_plant_frac: f32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            width: 256,
            height: 256,
            capacity: 12288,
            init_agents: 2000,
            mass_per_cell: 16.0,
            init_plant_frac: 0.35,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrainConfig {
    /// Радиус обзора: 3 — это окно 7x7, рабочий размер из обзора исследований.
    pub vision_radius: usize,
    /// Размеры скрытых слоёв.
    pub hidden: Vec<usize>,
    /// Возвращать ли выходы последнего скрытого слоя на вход следующего шага.
    /// Это даёт особи память между шагами: без неё она каждый раз
    /// решает заново, зная только то, что видит сейчас.
    pub recurrent: bool,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self { vision_radius: 3, hidden: vec![32, 32], recurrent: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub birth_energy: f32,
    pub max_energy: f32,
    /// Ниже этого порога особь умирает, а остаток её энергии становится падалью.
    pub death_energy: f32,
    /// Расход энергии за шаг просто на жизнь.
    pub metabolism: f32,
    pub move_cost: f32,
    /// Сколько массы особь способна съесть за шаг.
    pub eat_rate: f32,
    pub attack_damage: f32,
    pub attack_cost: f32,
    /// Доля нанесённого урона, достающаяся атакующему сразу; остальное падает
    /// падалью в клетку жертвы. При 0 атака не кормит вовсе, и связь между
    /// ударом и наградой приходится искать через ход — для отбора это слишком далеко.
    pub attack_yield: f32,
    pub repro_threshold: f32,
    /// Доля энергии родителя, уходящая потомку.
    pub repro_share: f32,
    /// Доля энергии родителя, теряемая при родах; возвращается в клетку.
    pub repro_overhead: f32,
    /// Старение: добавка к расходу за каждый шаг возраста.
    pub age_cost_rate: f32,
    /// Стандартное отклонение гауссовой мутации весов.
    pub mutation_sigma: f32,
    /// Ограничение весов по модулю; 0 отключает ограничение.
    pub weight_clamp: f32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            birth_energy: 40.0,
            max_energy: 120.0,
            death_energy: 1.0,
            metabolism: 0.35,
            move_cost: 0.15,
            eat_rate: 6.0,
            attack_damage: 60.0,
            attack_cost: 1.5,
            attack_yield: 0.8,
            repro_threshold: 80.0,
            repro_share: 0.4,
            repro_overhead: 0.08,
            age_cost_rate: 0.0006,
            mutation_sigma: 0.03,
            weight_clamp: 6.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceConfig {
    /// Скорость логистического роста растений.
    pub growth_rate: f32,
    /// Потолок растительной массы в клетке.
    pub plant_capacity: f32,
    /// Затравка роста: без неё выеденная дочиста клетка не восстановится.
    pub plant_seed: f32,
    /// Доля падали, разлагающейся в питательные вещества за шаг.
    pub carrion_decay: f32,
    /// Коэффициент диффузии питательных веществ, 0..0.2.
    pub nutrient_diffusion: f32,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            growth_rate: 0.12,
            plant_capacity: 8.0,
            plant_seed: 0.02,
            carrion_decay: 0.04,
            nutrient_diffusion: 0.08,
        }
    }
}

impl Config {
    /// Проверка заведомо бессмысленных значений: дешевле, чем ловить NaN на десятитысячном шаге.
    pub fn validate(&self) -> Result<(), String> {
        if self.world.width < 8 || self.world.height < 8 {
            return Err("мир меньше 8x8 не имеет смысла".into());
        }
        if self.world.width > u16::MAX as usize || self.world.height > u16::MAX as usize {
            return Err("координаты хранятся в u16: сторона мира не больше 65535".into());
        }
        if self.world.capacity == 0 || self.world.capacity >= u32::MAX as usize {
            return Err("capacity вне допустимого диапазона".into());
        }
        if self.world.init_agents > self.world.capacity {
            return Err("init_agents больше capacity".into());
        }
        if self.world.init_agents > self.world.width * self.world.height {
            return Err("особей больше, чем клеток: в клетке живёт не больше одной".into());
        }
        if self.brain.vision_radius == 0 || self.brain.vision_radius > 16 {
            return Err("vision_radius вне диапазона 1..16".into());
        }
        if self.brain.hidden.is_empty() || self.brain.hidden.iter().any(|&h| h == 0) {
            return Err("нужен хотя бы один непустой скрытый слой".into());
        }
        if self.agent.repro_share + self.agent.repro_overhead >= 1.0 {
            return Err("repro_share + repro_overhead должны быть меньше 1".into());
        }
        if !(0.0..=1.0).contains(&self.agent.attack_yield) {
            return Err("attack_yield вне диапазона 0..1".into());
        }
        if self.agent.repro_threshold <= self.agent.death_energy {
            return Err("repro_threshold должен быть выше death_energy".into());
        }
        if !(0.0..=0.2).contains(&self.resources.nutrient_diffusion) {
            return Err("nutrient_diffusion вне диапазона 0..0.2".into());
        }
        if self.resources.plant_capacity <= 0.0 {
            return Err("plant_capacity должен быть положительным".into());
        }
        Ok(())
    }
}
