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

/// Деление популяции по рациону. Не виды и не генетические группы:
/// особь попадает в ту или другую половину по тому, что съела за жизнь.
#[derive(Debug, Clone, Copy, Default)]
pub struct DietSplit {
    pub predators: usize,
    pub herbivores: usize,
    /// Съедено слишком мало, чтобы судить.
    pub undecided: usize,
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

    /// Сколько живых питаются преимущественно мясом, сколько растениями.
    pub predators: u64,
    pub herbivores: u64,
    /// Размер отсечённого дерева предков.
    pub phylo_nodes: u64,
    /// Сколько линий-основателей дожило.
    pub lineages: u64,
    /// Поколение последнего общего предка всех живых; имеет смысл при одной линии.
    pub mrca_generation: u32,
    pub mrca_step: u64,

    /// Ноль означает, что генетика на этом замере не считалась.
    pub genetics_sampled: u64,
    /// Расстояния — в мутационных шагах.
    pub mean_distance: f64,
    pub median_distance: f64,
    pub clusters: u64,
    pub largest_cluster_share: f64,
}

/// Запаздывающая корреляция: насколько `ys`, сдвинутый на `lag` замеров назад,
/// повторяет `xs`. В цикле Лотки–Вольтерры численность хищников повторяет
/// численность жертв с задержкой, поэтому пик корреляции ожидается не на нуле.
pub fn lagged_correlation(xs: &[f64], ys: &[f64], lag: usize) -> f64 {
    if lag >= xs.len() || xs.len() != ys.len() {
        return 0.0;
    }
    let a = &xs[..xs.len() - lag];
    let b = &ys[lag..];
    let n = a.len();
    if n < 3 {
        return 0.0;
    }
    let ma = a.iter().sum::<f64>() / n as f64;
    let mb = b.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for k in 0..n {
        let u = a[k] - ma;
        let v = b[k] - mb;
        num += u * v;
        da += u * u;
        db += v * v;
    }
    let den = (da * db).sqrt();
    if den > 0.0 {
        num / den
    } else {
        0.0
    }
}

/// Вычитает скользящее среднее.
///
/// Без этого корреляция двух рядов меряет их общий тренд, а не колебания:
/// пока охота только разворачивается, численность хищных растёт весь прогон,
/// и этот рост забивает любой цикл. Окно берётся нечётным и центрированным,
/// по краям сжимается.
pub fn detrend(series: &[f64], window: usize) -> Vec<f64> {
    let n = series.len();
    let half = window.max(1) / 2;
    if n == 0 || half == 0 {
        return series.to_vec();
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(n);
            let mean = series[lo..hi].iter().sum::<f64>() / (hi - lo) as f64;
            series[i] - mean
        })
        .collect()
}

/// Оценивает период колебаний по автокорреляции ряда.
///
/// Автокорреляция колеблющегося ряда сначала уходит в минус примерно
/// на полпериоде, а затем возвращается в плюс на полном периоде.
/// Возвращает задержку этого возврата и её корреляцию, либо `None`,
/// если ряд не колеблется: тогда минуса и возврата просто нет.
pub fn oscillation_period(series: &[f64], max_lag: usize) -> Option<(usize, f64)> {
    if series.len() < 8 {
        return None;
    }
    let max_lag = max_lag.min(series.len() / 2);
    let mut trough = (0usize, 0.0f64);
    for lag in 1..=max_lag {
        let c = lagged_correlation(series, series, lag);
        if c < trough.1 {
            trough = (lag, c);
        }
    }
    if trough.0 == 0 {
        return None;
    }
    let mut back = (0usize, 0.0f64);
    for lag in (trough.0 + 1)..=max_lag {
        let c = lagged_correlation(series, series, lag);
        if c > back.1 {
            back = (lag, c);
        }
    }
    if back.0 == 0 {
        None
    } else {
        Some(back)
    }
}

/// Ищет задержку с наибольшей корреляцией в диапазоне `0..=max_lag`.
///
/// Задержка на самом краю диапазона — обычно признак того, что найден
/// тренд, а не цикл: проверяйте, не упёрлось ли в `max_lag`.
pub fn best_lag(xs: &[f64], ys: &[f64], max_lag: usize) -> (usize, f64) {
    let mut best = (0usize, f64::NEG_INFINITY);
    for lag in 0..=max_lag.min(xs.len().saturating_sub(3)) {
        let c = lagged_correlation(xs, ys, lag);
        if c > best.1 {
            best = (lag, c);
        }
    }
    if best.1 == f64::NEG_INFINITY {
        (0, 0.0)
    } else {
        best
    }
}

const HEADER: &str = "step,population,mass_total,mass_agents,mass_plant,mass_carrion,mass_nutrient,\
agent_mass_frac,mean_energy,mean_age,max_generation,births,deaths_starved,deaths_killed,attacks,\
attacks_hit,kills_per_attack,hits_per_attack,kills_per_hit,carrion_share,animal_share,moves_per_agent,\
eats_per_agent,attacks_per_agent,eaten_per_agent,births_blocked,\
predators,herbivores,phylo_nodes,lineages,mrca_generation,mrca_step,\
genetics_sampled,mean_distance,median_distance,clusters,largest_cluster_share";

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
{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{:.4},{:.4},{},{:.4}",
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
            s.predators,
            s.herbivores,
            s.phylo_nodes,
            s.lineages,
            s.mrca_generation,
            s.mrca_step,
            s.genetics_sampled,
            s.mean_distance,
            s.median_distance,
            s.clusters,
            s.largest_cluster_share,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_matches_row_width() {
        // Заголовок и строка пишутся разными форматами, и однажды в заголовок
        // уже попал литерал переноса: CSV разъехался, а заметно это стало
        // только при разборе чужим инструментом.
        let mut buf = Vec::new();
        {
            let mut w = CsvWriter::new(&mut buf).unwrap();
            w.write(&Sample::default()).unwrap();
            w.flush().unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "заголовок и строка обязаны занимать по одной строке");
        assert_eq!(
            lines[0].split(',').count(),
            lines[1].split(',').count(),
            "колонок в заголовке {} против {} в строке",
            lines[0].split(',').count(),
            lines[1].split(',').count()
        );
    }

    #[test]
    fn correlation_of_a_series_with_itself_is_one() {
        let xs: Vec<f64> = (0..100).map(|i| (i as f64 * 0.3).sin()).collect();
        assert!((lagged_correlation(&xs, &xs, 0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn best_lag_finds_the_shift() {
        // Вторая кривая повторяет первую с отставанием в 7 замеров —
        // ровно та картина, которую ищут в цикле Лотки–Вольтерры.
        let n = 400;
        let shift = 7usize;
        let xs: Vec<f64> = (0..n).map(|i| (i as f64 * 0.05).sin()).collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| ((i as f64 - shift as f64) * 0.05).sin())
            .collect();
        let (lag, corr) = best_lag(&xs, &ys, 40);
        assert_eq!(lag, shift, "найдена задержка {lag}, корреляция {corr}");
        assert!(corr > 0.99, "корреляция на верной задержке всего {corr}");
    }

    #[test]
    fn detrending_removes_a_trend_but_keeps_the_cycle() {
        let n = 400;
        // Пила с периодом 40 поверх линейного роста.
        let raw: Vec<f64> = (0..n)
            .map(|i| i as f64 * 0.5 + (i as f64 * 0.157).sin() * 3.0)
            .collect();
        let flat = detrend(&raw, 41);
        let mean = flat.iter().sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.2, "после вычитания тренда среднее осталось {mean}");
        // Края трогать не надо: колебание должно уцелеть в середине.
        let amplitude = flat[100..300].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(amplitude > 1.5, "колебание съелось вместе с трендом: {amplitude}");
    }

    #[test]
    fn a_pure_trend_leaves_nothing_behind() {
        // Ряд, который просто растёт, без всякого цикла.
        let n = 300;
        let a: Vec<f64> = (0..n).map(|i| i as f64).collect();
        assert!(
            lagged_correlation(&a, &a, 0) > 0.99,
            "сам с собой тренд коррелирует идеально — это и есть ловушка"
        );

        let flat = detrend(&a, 31);
        // В середине ряда от линейного роста не остаётся ничего.
        let residual = flat[50..250].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(residual < 1e-9, "от тренда остался остаток {residual}");

        // По краям окно сжимается несимметрично, и остаток не нулевой.
        // Это цена метода: на коротком ряде края занимают заметную его часть.
        let edge = flat[0].abs();
        assert!(edge > 0.0, "края обязаны отличаться от середины");
    }

    #[test]
    fn oscillation_period_finds_a_known_period() {
        let period = 40.0;
        let xs: Vec<f64> = (0..600)
            .map(|i| (i as f64 * std::f64::consts::TAU / period).sin())
            .collect();
        let (lag, corr) = oscillation_period(&xs, 120).expect("период не найден");
        assert!(
            (lag as f64 - period).abs() <= 2.0,
            "период оценён как {lag} вместо {period}"
        );
        assert!(corr > 0.8, "возврат автокорреляции всего {corr}");
    }

    #[test]
    fn a_series_without_oscillation_has_no_period() {
        // Монотонный ряд: автокорреляция только спадает, в минус не уходит.
        let xs: Vec<f64> = (0..200).map(|i| i as f64).collect();
        assert!(oscillation_period(&xs, 50).is_none());
        assert!(oscillation_period(&[1.0, 2.0, 3.0], 2).is_none(), "ряд слишком короткий");
    }

    #[test]
    fn complementary_series_mirror_each_other() {
        // Две доли, которые в сумме дают постоянную: так устроены хищные
        // и травоядные, пока они — одна популяция. Корреляция таких рядов
        // равна автокорреляции со знаком минус, и «задержка» у них всегда
        // приходится на полпериода, что бы ни происходило на самом деле.
        let period = 40.0;
        let a: Vec<f64> = (0..400)
            .map(|i| (i as f64 * std::f64::consts::TAU / period).sin())
            .collect();
        let b: Vec<f64> = a.iter().map(|v| -v).collect();
        assert!((lagged_correlation(&a, &b, 0) + 1.0).abs() < 1e-9);
        let (lag, corr) = best_lag(&a, &b, 60);
        assert!(corr > 0.9);
        assert!(
            (lag as f64 - period / 2.0).abs() <= 2.0,
            "пик пришёлся на {lag}, а полпериода — {}",
            period / 2.0
        );
    }

    #[test]
    fn correlation_is_zero_without_data() {
        assert_eq!(lagged_correlation(&[1.0, 2.0], &[1.0, 2.0], 5), 0.0);
        assert_eq!(lagged_correlation(&[1.0; 10], &[1.0; 10], 0), 0.0, "у плоского ряда нет разброса");
        assert_eq!(best_lag(&[], &[], 10), (0, 0.0));
    }
}
