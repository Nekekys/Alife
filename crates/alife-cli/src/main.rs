//! Пакетный прогон симуляции без отрисовки.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use alife_core::{Config, CsvWriter, Sim};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "alife", about = "Экосистема с нейросетевыми особями", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Прогнать симуляцию и записать метрики в CSV.
    Run {
        /// Файл конфигурации; без него берутся значения по умолчанию.
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value_t = 20_000)]
        steps: u64,
        /// Сид поверх указанного в конфиге.
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long, default_value = "runs/run.csv")]
        out: PathBuf,
        /// Как часто снимать замер, в шагах.
        #[arg(long, default_value_t = 100)]
        every: u64,
        /// Число потоков; 0 — по числу ядер.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Остановиться, когда популяция вымерла.
        #[arg(long, default_value_t = true)]
        stop_on_extinction: bool,
        /// Как часто считать генетику, в шагах; 0 отключает.
        /// Проекция геномов на порядок дороже остальных метрик.
        #[arg(long, default_value_t = 2000)]
        genetics_every: u64,
        /// Сколько особей брать в выборку для генетики.
        #[arg(long, default_value_t = 512)]
        genetics_sample: usize,
        /// Порог склейки кластеров, в мутационных шагах.
        #[arg(long, default_value_t = 20.0)]
        cluster_threshold: f64,
    },
    /// Разобрать готовый CSV и повторить выводы о динамике без пересчёта.
    Report {
        #[arg(long)]
        csv: PathBuf,
    },
    /// Вывести конфигурацию по умолчанию.
    Config {
        /// Куда записать; без аргумента — в стандартный вывод.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ошибка: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match Cli::parse().cmd {
        Cmd::Report { csv } => {
            let text = fs::read_to_string(&csv).map_err(|e| format!("{}: {e}", csv.display()))?;
            let mut lines = text.lines();
            let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
            let col = |name: &str| {
                header
                    .iter()
                    .position(|h| *h == name)
                    .ok_or_else(|| format!("в {} нет колонки {name}", csv.display()))
            };
            let (i_step, i_pop, i_pred, i_herb) =
                (col("step")?, col("population")?, col("predators")?, col("herbivores")?);

            let (mut steps, mut population, mut predators, mut herbivores) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for line in lines {
                let f: Vec<&str> = line.split(',').collect();
                if f.len() <= i_herb {
                    continue;
                }
                let parse = |i: usize| f[i].parse::<f64>().unwrap_or(0.0);
                steps.push(parse(i_step) as u64);
                population.push(parse(i_pop).max(1.0));
                predators.push(parse(i_pred));
                herbivores.push(parse(i_herb));
            }
            if steps.len() < 2 {
                return Err(format!("{}: нечего разбирать", csv.display()));
            }
            let every = steps[1].saturating_sub(steps[0]).max(1);
            eprintln!("{}: {} замеров через {} шагов", csv.display(), steps.len(), every);
            report_dynamics(&population, &herbivores, &predators, every);
            Ok(())
        }
        Cmd::Config { out } => {
            let text = toml::to_string_pretty(&Config::default())
                .map_err(|e| format!("не удалось сериализовать конфиг: {e}"))?;
            match out {
                Some(path) => {
                    write_parents(&path)?;
                    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                    eprintln!("записано: {}", path.display());
                }
                None => println!("{text}"),
            }
            Ok(())
        }
        Cmd::Run {
            config,
            steps,
            seed,
            out,
            every,
            threads,
            stop_on_extinction,
            genetics_every,
            genetics_sample,
            cluster_threshold,
        } => {
            let mut cfg = match config {
                Some(path) => {
                    let text = fs::read_to_string(&path)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    toml::from_str::<Config>(&text)
                        .map_err(|e| format!("{}: {e}", path.display()))?
                }
                None => Config::default(),
            };
            if let Some(s) = seed {
                cfg.seed = s;
            }
            if threads > 0 {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build_global()
                    .map_err(|e| format!("не удалось настроить пул потоков: {e}"))?;
            }

            let mut sim = Sim::new(cfg)?;
            eprintln!(
                "мир {}x{}, ёмкость {}, особей на старте {}",
                sim.world.w,
                sim.world.h,
                sim.agents.capacity,
                sim.population()
            );
            eprintln!(
                "сеть: {} входов -> {:?} -> {} выходов, {} весов на особь, геномы занимают {:.1} МБ",
                sim.brain.n_in,
                sim.cfg.brain.hidden,
                sim.brain.n_out,
                sim.brain.genome_len,
                sim.genome_bytes() as f64 / (1024.0 * 1024.0),
            );
            eprintln!("масса мира: {:.1}", sim.initial_mass());

            write_parents(&out)?;
            let file = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            let mut csv = CsvWriter::new(BufWriter::new(file))
                .map_err(|e| format!("{}: {e}", out.display()))?;

            let started = Instant::now();
            let mut last_report = Instant::now();
            let mut extinct_at = None;

            // Замер обнуляет счётчики интервала, поэтому лишний замер сразу после
            // предыдущего дал бы строку с нулями во всех метриках событий.
            let mut sampled_last_tick = false;
            // Ряды для проверки цикла Лотки–Вольтерры: численность хищников
            // должна повторять численность жертв с задержкой.
            let mut herbivores: Vec<f64> = Vec::new();
            let mut predators: Vec<f64> = Vec::new();
            let mut population: Vec<f64> = Vec::new();
            for step in 1..=steps {
                sim.tick();
                sampled_last_tick = false;
                if step % every == 0 || step == steps {
                    let with_genetics = genetics_every > 0 && step % genetics_every == 0;
                    let s = if with_genetics {
                        sim.sample_full(genetics_sample, cluster_threshold)
                    } else {
                        sim.sample()
                    };
                    herbivores.push(s.herbivores as f64);
                    predators.push(s.predators as f64);
                    population.push(s.population.max(1) as f64);
                    csv.write(&s).map_err(|e| format!("{}: {e}", out.display()))?;
                    sampled_last_tick = true;
                    if last_report.elapsed().as_secs_f64() > 1.0 || step == steps {
                        eprintln!("{}", s.summary());
                        last_report = Instant::now();
                    }
                }
                if stop_on_extinction && sim.extinct() {
                    extinct_at = Some(step);
                    break;
                }
            }

            if !sampled_last_tick {
                let s = sim.sample();
                csv.write(&s).map_err(|e| format!("{}: {e}", out.display()))?;
            }
            csv.flush().map_err(|e| format!("{}: {e}", out.display()))?;

            let secs = started.elapsed().as_secs_f64();
            let done = extinct_at.unwrap_or(steps);
            eprintln!("---");
            if let Some(step) = extinct_at {
                eprintln!("популяция вымерла на шаге {step}");
            }
            eprintln!(
                "{done} шагов за {secs:.1} с: {:.1} шагов/с",
                done as f64 / secs.max(1e-9)
            );
            let drift = (sim.total_mass() - sim.initial_mass()).abs() / sim.initial_mass();
            eprintln!("уход массы за прогон: {:.2e}", drift);

            let ph = sim.phylo_stats();
            eprintln!(
                "филогения: {} узлов, выживших линий {}, общий предок в поколении {} (шаг {})",
                ph.nodes, ph.roots, ph.mrca_generation, ph.mrca_step
            );
            if genetics_every > 0 {
                let thresholds = [2.0, 5.0, 10.0, 20.0, 40.0];
                let scan = sim.genetics_scan(genetics_sample, &thresholds);
                if let Some(g) = scan.first() {
                    eprintln!(
                        "разнообразие по {} особям: расстояние между геномами {:.1} мутационных шагов (медиана {:.1}, от {:.1} до {:.1})",
                        g.sampled, g.mean_distance, g.median_distance, g.p10_distance, g.p90_distance
                    );
                }
                let parts: Vec<String> = scan
                    .iter()
                    .map(|g| {
                        format!(
                            "{:.0} -> {} ({:.0}%)",
                            g.threshold,
                            g.clusters,
                            g.largest_cluster_share * 100.0
                        )
                    })
                    .collect();
                eprintln!("кластеров при порогах (в крупнейшем): {}", parts.join(", "));
            }
            report_dynamics(&population, &herbivores, &predators, every);
            eprintln!("метрики: {}", out.display());
            Ok(())
        }
    }
}

/// Разбор колебаний численности. Вынесен отдельно, чтобы одни и те же выводы
/// делались и по свежему прогону, и по сохранённому CSV.
fn report_dynamics(population: &[f64], herbivores: &[f64], predators: &[f64], every: u64) {
    if herbivores.len() <= 40 {
        eprintln!("замеров слишком мало, чтобы судить о колебаниях");
        return;
    }
    // Ряды нестационарны, пока охота разворачивается: численность хищных
    // растёт весь прогон. Без вычитания тренда корреляция меряет этот рост.
    let window = (herbivores.len() / 8).max(5) | 1;
    let flat_h = alife_core::detrend(herbivores, window);
    let flat_p = alife_core::detrend(predators, window);
    let max_lag = herbivores.len() / 8;
    // Период ищется в более широком окне: искомая величина заведомо больше
    // задержек, которые имеет смысл перебирать в кросс-корреляции.
    let period = alife_core::oscillation_period(&flat_p, herbivores.len() / 3);
    match period {
        Some((lag, corr)) => eprintln!(
            "численность хищных колеблется: период около {} шагов (автокорреляция возвращается к {:.2})",
            lag as u64 * every,
            corr
        ),
        None => eprintln!("численность хищных не колеблется: периода не видно"),
    }

    // Хищные и травоядные делят одну популяцию, поэтому их доли могут
    // оказаться почти зеркальными. У зеркальных рядов пик кросс-корреляции
    // всегда приходится на полпериода, что бы ни происходило на самом деле.
    let share = |xs: &[f64]| -> Vec<f64> {
        xs.iter().zip(population).map(|(x, p)| x / p).collect()
    };
    let mirror = alife_core::lagged_correlation(
        &alife_core::detrend(&share(herbivores), window),
        &alife_core::detrend(&share(predators), window),
        0,
    );
    eprintln!("зеркальность долей хищных и травоядных: {mirror:.2}");

    let (lag, corr) = alife_core::best_lag(&flat_h, &flat_p, max_lag);
    let mut caveats: Vec<&str> = Vec::new();
    if lag == 0 || lag == max_lag {
        caveats.push("задержка упёрлась в край окна поиска");
    }
    if period.is_some_and(|(p, _)| {
        (lag as f64 - p as f64 / 2.0).abs() <= (p as f64 * 0.2).max(1.0)
    }) {
        caveats.push("это полпериода, то есть следствие зеркальности");
    }
    if mirror < -0.8 && caveats.is_empty() {
        caveats.push("доли почти зеркальны, о причинности судить нельзя");
    }
    let tail = if caveats.is_empty() {
        String::new()
    } else {
        format!(" — {}", caveats.join("; "))
    };
    eprintln!(
        "жертвы против хищников: задержка {} шагов, корреляция {:.3}{}",
        lag as u64 * every,
        corr,
        tail
    );
}

fn write_parents(path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    Ok(())
}
