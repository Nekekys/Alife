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
        Cmd::Run { config, steps, seed, out, every, threads, stop_on_extinction } => {
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
            for step in 1..=steps {
                sim.tick();
                sampled_last_tick = false;
                if step % every == 0 || step == steps {
                    let s = sim.sample();
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
            eprintln!("метрики: {}", out.display());
            Ok(())
        }
    }
}

fn write_parents(path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    Ok(())
}
