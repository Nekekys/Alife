//! Окно наблюдения за экосистемой: карта мира, графики метрик и инспектор особи.
//!
//! Авторы работы 2025 года прямо пишут, что встроенный сбор статистики и
//! просмотрщик с инспектором особей были ключевыми для обнаружения поведения,
//! которого не видно по одним числам. Поэтому наблюдение строится раньше
//! сложных механик.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod charts;
mod colors;
mod inspector;
mod worldview;

use std::path::PathBuf;

use alife_core::Config;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut cfg_path: Option<PathBuf> = None;
    let mut seed: Option<u64> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => cfg_path = args.next().map(PathBuf::from),
            "--seed" => seed = args.next().and_then(|s| s.parse().ok()),
            "--help" | "-h" => {
                println!("alife-viewer [--config <файл.toml>] [--seed <число>]");
                return Ok(());
            }
            other => {
                eprintln!("неизвестный аргумент: {other}");
                std::process::exit(2);
            }
        }
    }

    let mut cfg = match &cfg_path {
        Some(path) => match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<Config>(&t).map_err(|e| e.to_string()))
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                std::process::exit(1);
            }
        },
        None => Config::default(),
    };
    if let Some(s) = seed {
        cfg.seed = s;
    }

    let app = match app::ViewerApp::new(cfg, cfg_path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ошибка: {e}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("alife — просмотрщик"),
        ..Default::default()
    };

    eframe::run_native("alife", options, Box::new(move |_cc| Ok(Box::new(app))))
}
