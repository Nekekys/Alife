//! Сбор статистики. Наблюдение ставится раньше сложных механик:
//! без него непонятно, почему экосистема схлопнулась.

use std::io::{self, Write};

/// Счётчики событий, накопленные с прошлого замера.
#[derive(Debug, Clone, Copy, Default)]
pub struct Counters {
    pub births: u64,
    pub deaths_starved: u64,
    pub deaths_killed: u64,
    pub attacks: u64,
    pub moves: u64,
    pub eats: u64,
    pub eaten_mass: f64,
    /// Ёмкость кончилась, размножение не состоялось: признак упора в потолок.
    pub births_blocked: u64,
    /// Число шагов, вошедших в этот замер.
    pub ticks: u64,
    /// Сумма числа живых особей по шагам: знаменатель для метрик «на особь».
    pub agent_ticks: u64,
}

impl Counters {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub step: u64,
    pub population: u64,
    pub mass_total: f64,
    pub mass_agents: f64,
    pub mass_plant: f64,
    pub mass_carrion: f64,
    pub mass_nutrient: f64,
    /// Доля биомассы внутри особей — метрика из работы Bejjani.
    pub agent_mass_frac: f64,
    pub mean_energy: f64,
    pub mean_age: f64,
    pub max_generation: u32,
    pub births: u64,
    pub deaths_starved: u64,
    pub deaths_killed: u64,
    pub attacks: u64,
    /// Смертей на одну атаку: возникла ли специализированная охота.
    pub kills_per_attack: f64,
    pub moves_per_agent: f64,
    pub eats_per_agent: f64,
    pub attacks_per_agent: f64,
    pub eaten_per_agent: f64,
    pub births_blocked: u64,
}

const HEADER: &str = "step,population,mass_total,mass_agents,mass_plant,mass_carrion,mass_nutrient,\
agent_mass_frac,mean_energy,mean_age,max_generation,births,deaths_starved,deaths_killed,attacks,\
kills_per_attack,moves_per_agent,eats_per_agent,attacks_per_agent,eaten_per_agent,births_blocked";

/// Запись замеров в CSV.
pub struct CsvWriter<W: Write> {
    inner: W,
}

impl<W: Write> CsvWriter<W> {
    pub fn new(mut inner: W) -> io::Result<Self> {
        writeln!(inner, "{HEADER}")?;
        Ok(Self { inner })
    }

    pub fn write(&mut self, s: &Sample) -> io::Result<()> {
        writeln!(
            self.inner,
            "{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.6},{:.4},{:.2},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{}",
            s.step,
            s.population,
            s.mass_total,
            s.mass_agents,
            s.mass_plant,
            s.mass_carrion,
            s.mass_nutrient,
            s.agent_mass_frac,
            s.mean_energy,
            s.mean_age,
            s.max_generation,
            s.births,
            s.deaths_starved,
            s.deaths_killed,
            s.attacks,
            s.kills_per_attack,
            s.moves_per_agent,
            s.eats_per_agent,
            s.attacks_per_agent,
            s.eaten_per_agent,
            s.births_blocked,
        )
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Sample {
    /// Короткая строка для вывода в консоль по ходу прогона.
    pub fn summary(&self) -> String {
        format!(
            "шаг {:>8} | особей {:>6} | в особях {:>5.1}% | энергия {:>6.1} | возраст {:>6.1} | \
поколение {:>4} | рождений {:>5} | смертей {:>5} (убито {:>4}) | убийств/атаку {:.3}",
            self.step,
            self.population,
            self.agent_mass_frac * 100.0,
            self.mean_energy,
            self.mean_age,
            self.max_generation,
            self.births,
            self.deaths_starved + self.deaths_killed,
            self.deaths_killed,
            self.kills_per_attack,
        )
    }
}
