//! «Мозг» особи: полносвязная сеть, веса которой лежат одним плоским
//! срезом генома. Раскладка весов общая для всей популяции, поэтому
//! геномы хранятся в одном большом массиве (см. [`crate::agents::Agents`]).

use crate::rng::Rng;

/// Выходы сети: [стоять, север, юг, запад, восток, есть, атаковать, размножиться].
pub const N_OUTPUTS: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct Layer {
    pub w_off: usize,
    pub b_off: usize,
    pub n_in: usize,
    pub n_out: usize,
}

#[derive(Debug, Clone)]
pub struct BrainSpec {
    pub n_in: usize,
    pub n_out: usize,
    pub layers: Vec<Layer>,
    pub genome_len: usize,
    pub max_width: usize,
}

impl BrainSpec {
    pub fn new(n_in: usize, hidden: &[usize], n_out: usize) -> Self {
        let mut layers = Vec::with_capacity(hidden.len() + 1);
        let mut off = 0usize;
        let mut prev = n_in;
        let mut max_width = n_in.max(n_out);
        for &h in hidden {
            let w_off = off;
            off += prev * h;
            let b_off = off;
            off += h;
            layers.push(Layer { w_off, b_off, n_in: prev, n_out: h });
            prev = h;
            max_width = max_width.max(h);
        }
        let w_off = off;
        off += prev * n_out;
        let b_off = off;
        off += n_out;
        layers.push(Layer { w_off, b_off, n_in: prev, n_out });
        Self { n_in, n_out, layers, genome_len: off, max_width }
    }

    /// Случайный геном: веса N(0, 1/sqrt(n_in)), смещения нулевые.
    pub fn random_genome(&self, rng: &mut Rng, out: &mut [f32]) {
        debug_assert_eq!(out.len(), self.genome_len);
        for l in &self.layers {
            let sigma = 1.0 / (l.n_in as f32).sqrt();
            rng.fill_gaussian(&mut out[l.w_off..l.w_off + l.n_in * l.n_out], sigma);
            for b in &mut out[l.b_off..l.b_off + l.n_out] {
                *b = 0.0;
            }
        }
    }

    /// Прямой проход. Результат остаётся в буфере `s` и возвращается срезом.
    pub fn forward<'a>(&self, g: &[f32], input: &[f32], s: &'a mut Scratch) -> &'a [f32] {
        debug_assert_eq!(input.len(), self.n_in);
        s.a.clear();
        s.a.extend_from_slice(input);
        let last = self.layers.len() - 1;
        for (i, l) in self.layers.iter().enumerate() {
            s.b.clear();
            s.b.resize(l.n_out, 0.0);
            let w = &g[l.w_off..l.w_off + l.n_in * l.n_out];
            let bias = &g[l.b_off..l.b_off + l.n_out];
            matvec(w, bias, &s.a, &mut s.b, l.n_in);
            if i != last {
                for v in s.b.iter_mut() {
                    *v = tanh_approx(*v);
                }
            }
            std::mem::swap(&mut s.a, &mut s.b);
        }
        &s.a
    }
}

/// Буферы прямого прохода, переиспользуемые внутри одного потока.
#[derive(Debug, Clone, Default)]
pub struct Scratch {
    a: Vec<f32>,
    b: Vec<f32>,
}

impl Scratch {
    pub fn new(width: usize) -> Self {
        Self { a: Vec::with_capacity(width), b: Vec::with_capacity(width) }
    }
}

#[inline]
fn matvec(w: &[f32], bias: &[f32], x: &[f32], out: &mut [f32], n_in: usize) {
    for (j, o) in out.iter_mut().enumerate() {
        let row = &w[j * n_in..(j + 1) * n_in];
        let mut acc = 0.0f32;
        for (wi, xi) in row.iter().zip(x.iter()) {
            acc += wi * xi;
        }
        *o = acc + bias[j];
    }
}

/// Паде-аппроксимация tanh. Системный `tanh` может отличаться между
/// платформами, а прогоны должны совпадать побитово.
#[inline]
fn tanh_approx(x: f32) -> f32 {
    let x = x.clamp(-8.0, 8.0);
    let x2 = x * x;
    let num = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let den = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + x2 * 28.0));
    (num / den).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genome_layout_is_contiguous() {
        let spec = BrainSpec::new(10, &[4, 3], 2);
        assert_eq!(spec.genome_len, 10 * 4 + 4 + 4 * 3 + 3 + 3 * 2 + 2);
        assert_eq!(spec.max_width, 10);
    }

    #[test]
    fn tanh_approx_is_close_to_tanh() {
        for i in -60..=60 {
            let x = i as f32 * 0.1;
            assert!((tanh_approx(x) - x.tanh()).abs() < 1e-4, "x = {x}");
        }
    }

    #[test]
    fn forward_runs_and_stays_finite() {
        let spec = BrainSpec::new(6, &[5, 5], N_OUTPUTS);
        let mut rng = Rng::new(7);
        let mut g = vec![0.0; spec.genome_len];
        spec.random_genome(&mut rng, &mut g);
        let mut s = Scratch::new(spec.max_width);
        let out = spec.forward(&g, &[0.5; 6], &mut s);
        assert_eq!(out.len(), N_OUTPUTS);
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
