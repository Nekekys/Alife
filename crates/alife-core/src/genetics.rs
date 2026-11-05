//! Генетическое расстояние и кластеры.
//!
//! Сравнивать геномы попарно напрямую нельзя: при тысячах особей и тысячах
//! весов это миллиарды операций на один замер. Поэтому каждый геном сначала
//! сжимается случайной проекцией в короткую подпись, в которой евклидовы
//! расстояния сохраняются (лемма Джонсона — Линденштрауса), а кластеры
//! считаются уже по подписям.

use rayon::prelude::*;

use crate::rng::Rng;

/// Случайная проекция генома в короткую подпись.
#[derive(Debug, Clone)]
pub struct Projector {
    pub dim: usize,
    pub genome_len: usize,
    /// `dim` строк длиной `genome_len`, элементы ±1/sqrt(dim).
    rows: Vec<f32>,
}

impl Projector {
    pub fn new(genome_len: usize, dim: usize, seed: u64) -> Self {
        let scale = 1.0 / (dim as f32).sqrt();
        let mut rng = Rng::new(seed);
        let mut rows = vec![0.0f32; dim * genome_len];
        // Знаковая матрица Ахлиоптаса: сохраняет расстояния не хуже гауссовой,
        // а считается сложением и вычитанием.
        for v in rows.iter_mut() {
            *v = if rng.next_u64() & 1 == 0 { scale } else { -scale };
        }
        Self { dim, genome_len, rows }
    }

    pub fn project(&self, genome: &[f32], out: &mut [f32]) {
        debug_assert_eq!(genome.len(), self.genome_len);
        debug_assert_eq!(out.len(), self.dim);
        for (j, o) in out.iter_mut().enumerate() {
            let row = &self.rows[j * self.genome_len..(j + 1) * self.genome_len];
            let mut acc = 0.0f32;
            for (w, g) in row.iter().zip(genome.iter()) {
                acc += w * g;
            }
            *o = acc;
        }
    }
}

/// Сводка по разнообразию популяции на один замер.
#[derive(Debug, Clone, Default)]
pub struct Genetics {
    /// Сколько особей попало в выборку.
    pub sampled: usize,
    /// Расстояния между геномами — в единицах одного мутационного шага,
    /// то есть «на сколько мутаций в среднем разошлись две случайные особи».
    pub mean_distance: f64,
    pub median_distance: f64,
    pub p10_distance: f64,
    pub p90_distance: f64,
    /// Порог, по которому склеивались кластеры, в тех же единицах.
    pub threshold: f64,
    pub clusters: usize,
    /// Доля выборки в самом большом кластере: единица означает, что
    /// одна стратегия захватила мир.
    pub largest_cluster_share: f64,
}

/// Считает расстояния и кластеры по выборке подписей.
///
/// `sketches` — подписи подряд, по `dim` чисел на особь. `mutation_step` —
/// ожидаемое расстояние, которое даёт одна мутация; в нём и меряются
/// расстояния, иначе числа зависели бы от длины генома и ничего не говорили.
pub fn analyse(sketches: &[f32], dim: usize, mutation_step: f32, threshold_steps: f64) -> Genetics {
    let n = if dim == 0 { 0 } else { sketches.len() / dim };
    if n < 2 {
        return Genetics { sampled: n, threshold: threshold_steps, clusters: n, ..Default::default() };
    }

    let step = mutation_step.max(1e-9) as f64;
    let threshold = threshold_steps * step;

    let mut parent: Vec<usize> = (0..n).collect();
    let mut distances = Vec::with_capacity(n * (n - 1) / 2);

    for i in 0..n {
        let a = &sketches[i * dim..(i + 1) * dim];
        for j in (i + 1)..n {
            let b = &sketches[j * dim..(j + 1) * dim];
            let mut sum = 0.0f32;
            for (x, y) in a.iter().zip(b.iter()) {
                let d = x - y;
                sum += d * d;
            }
            let d = sum.sqrt() as f64;
            distances.push(d);
            // Связная компонента по порогу: кластер — это группа, внутри
            // которой до соседа не дальше порога.
            if d <= threshold {
                union(&mut parent, i, j);
            }
        }
    }

    distances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = distances.iter().sum::<f64>() / distances.len() as f64;

    let mut sizes = std::collections::HashMap::new();
    for i in 0..n {
        *sizes.entry(find(&mut parent, i)).or_insert(0usize) += 1;
    }
    let largest = sizes.values().copied().max().unwrap_or(0);

    Genetics {
        sampled: n,
        mean_distance: mean / step,
        median_distance: quantile(&distances, 0.5) / step,
        p10_distance: quantile(&distances, 0.1) / step,
        p90_distance: quantile(&distances, 0.9) / step,
        threshold: threshold_steps,
        clusters: sizes.len(),
        largest_cluster_share: largest as f64 / n as f64,
    }
}

/// Считает подписи для набора слотов параллельно.
pub fn sketch_all(
    projector: &Projector,
    genomes: &[f32],
    genome_len: usize,
    slots: &[u32],
) -> Vec<f32> {
    let dim = projector.dim;
    let mut out = vec![0.0f32; slots.len() * dim];
    out.par_chunks_mut(dim).enumerate().for_each(|(k, chunk)| {
        let slot = slots[k] as usize;
        let g = &genomes[slot * genome_len..(slot + 1) * genome_len];
        projector.project(g, chunk);
    });
    out
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[rb] = ra;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_roughly_preserves_distance() {
        // Проекция нужна только затем, чтобы расстояния пережили сжатие.
        let len = 2000;
        let dim = 128;
        let p = Projector::new(len, dim, 1);
        let mut rng = Rng::new(42);
        let mut a = vec![0.0f32; len];
        let mut b = vec![0.0f32; len];
        rng.fill_gaussian(&mut a, 1.0);
        rng.fill_gaussian(&mut b, 1.0);

        let full: f32 = a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt();

        let mut pa = vec![0.0f32; dim];
        let mut pb = vec![0.0f32; dim];
        p.project(&a, &mut pa);
        p.project(&b, &mut pb);
        let short: f32 = pa.iter().zip(&pb).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt();

        let error = (short - full).abs() / full;
        assert!(error < 0.2, "расстояние уехало на {error}: {full} против {short}");
    }

    #[test]
    fn identical_genomes_collapse_into_one_cluster() {
        let dim = 8;
        let n = 10;
        let sketches = vec![0.5f32; n * dim];
        let g = analyse(&sketches, dim, 1.0, 1.0);
        assert_eq!(g.sampled, n);
        assert_eq!(g.clusters, 1);
        assert_eq!(g.largest_cluster_share, 1.0);
        assert_eq!(g.mean_distance, 0.0);
    }

    #[test]
    fn far_apart_groups_are_counted_separately() {
        let dim = 4;
        // Три группы по три особи, разнесённые заведомо дальше порога.
        let mut sketches = Vec::new();
        for group in 0..3 {
            for _ in 0..3 {
                for _ in 0..dim {
                    sketches.push(group as f32 * 100.0);
                }
            }
        }
        let g = analyse(&sketches, dim, 1.0, 5.0);
        assert_eq!(g.sampled, 9);
        assert_eq!(g.clusters, 3);
        assert!((g.largest_cluster_share - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn distances_are_measured_in_mutation_steps() {
        let dim = 1;
        let sketches = vec![0.0f32, 10.0];
        // При шаге мутации 2 расстояние 10 — это пять мутационных шагов.
        let g = analyse(&sketches, dim, 2.0, 1.0);
        assert!((g.mean_distance - 5.0).abs() < 1e-6, "вышло {}", g.mean_distance);
        assert_eq!(g.clusters, 2, "порог в один шаг не должен их склеить");
    }
}
