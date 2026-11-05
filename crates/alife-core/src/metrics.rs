//! Сбор статистики. Наблюдение ставится раньше сложных механик:
//! без него непонятно, почему экосистема схлопнулась.

use std::io::{self, Write};

/// Счётчики событий, накопленные с прошлого замера.
#[derive(Debug, Clone, Copy, Default)]
pub struct Counters {
    pub births: u64,
    pub deaths_starved: u64,
    pub deaths_killed: u64,
    /// Попыток атаки, включая удары по пустой клетке.
    pub attacks: u64,
    /// Атак, пришедшихся по занятой клетке.
    pub attacks_hit: u64,
    pub damage_dealt: f64,
    /// Масса, снятая с жертвы укусом напрямую.
    pub predated_mass: f64,
    pub moves: u64,
    pub eats: u64,
    pub eaten_plant: f64,
    pub eaten_carrion: f64,
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
    pub attacks_hit: u64,
    /// Смертей на одну атаку: возникла ли специализированная охота.
    pub kills_per_attack: f64,
    /// Доля атак, попавших по особи: умеет ли популяция вообще наводиться.
    pub hits_per_attack: f64,
    /// Летальность попадания. У Bejjani около 75% атак зрячих особей были смертельными.
    pub kills_per_hit: f64,
    /// Доля падали в съеденном с земли.
    pub carrion_share: f64,
    /// Доля животной пищи во всём съеденном — укусы и падаль вместе.
    /// Главный признак того, что охота кормит.
    pub animal_share: f64,
    pub moves_per_agent: f64,
    pub eats_per_agent: f64,
    pub attacks_per_agent: f64,
    pub eaten_per_agent: f64,
    pub births_blocked: u64,
}

const HEADER: &str = "step,population,mass_total,mass_agents,mass_plant,mass_carrion,mass_nutrient,\
agent_mass_frac,mean_energy,mean_age,max_generation,births,deaths_starved,deaths_killed,attacks,\
attacks_hit,kills_per_attack,hits_per_attack,kills_per_hit,carrion_share,animal_share,moves_per_agent,\
eats_per_agent,attacks_per_agent,eaten_per_agent,births_blocked";

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
            "{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.6},{:.4},{:.2},{},{},{},{},{},{},\
{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{}",
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
            s.attacks_hit,
            s.kills_per_attack,
            s.hits_per_attack,
            s.kills_per_hit,
            s.carrion_share,
            s.animal_share,
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
поколение {:>4} | рождений {:>5} | смертей {:>5} (убито {:>4}) | попаданий/атаку {:.3} | \
убийств/попадание {:.3} | животной пищи {:>5.1}%",
            self.step,
            self.population,
            self.agent_mass_frac * 100.0,
            self.mean_energy,
            self.mean_age,
            self.max_generation,
            self.births,
            self.deaths_starved + self.deaths_killed,
            self.deaths_killed,
            self.hits_per_attack,
            self.kills_per_hit,
            self.animal_share * 100.0,
        )
    }
}
