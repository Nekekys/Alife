//! Сетка мира. Масса живёт в трёх пулах клетки и в энергии особей;
//! сумма всех четырёх постоянна весь прогон.

use crate::config::ResourceConfig;

/// Значение в `occupant`, означающее пустую клетку.
pub const NO_AGENT: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct World {
    pub w: usize,
    pub h: usize,
    /// Съедобная растительная масса.
    pub plant: Vec<f32>,
    /// Останки: съедаются первыми, иначе разлагаются.
    pub carrion: Vec<f32>,
    /// Несъедобный пул, из которого растут растения.
    pub nutrient: Vec<f32>,
    /// Индекс особи в клетке или [`NO_AGENT`]. В клетке не больше одной особи.
    pub occupant: Vec<u32>,
    diff_buf: Vec<f32>,
}

impl World {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            w,
            h,
            plant: vec![0.0; n],
            carrion: vec![0.0; n],
            nutrient: vec![0.0; n],
            occupant: vec![NO_AGENT; n],
            diff_buf: vec![0.0; n],
        }
    }

    #[inline]
    pub fn cells(&self) -> usize {
        self.w * self.h
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    #[inline]
    pub fn idx_wrap(&self, x: i32, y: i32) -> usize {
        let xx = x.rem_euclid(self.w as i32) as usize;
        let yy = y.rem_euclid(self.h as i32) as usize;
        yy * self.w + xx
    }

    /// Шаг на одну клетку по тору. Направления: 0 север, 1 юг, 2 запад, 3 восток.
    #[inline]
    pub fn step_wrap(&self, x: u16, y: u16, dir: u8) -> (u16, u16) {
        let w = self.w as u16;
        let h = self.h as u16;
        match dir {
            0 => (x, if y == 0 { h - 1 } else { y - 1 }),
            1 => (x, if y + 1 == h { 0 } else { y + 1 }),
            2 => (if x == 0 { w - 1 } else { x - 1 }, y),
            _ => (if x + 1 == w { 0 } else { x + 1 }, y),
        }
    }

    /// Масса, лежащая в клетках, без энергии особей.
    pub fn cell_mass(&self) -> f64 {
        let (p, c, n) = self.pool_totals();
        p + c + n
    }

    pub fn pool_totals(&self) -> (f64, f64, f64) {
        let mut p = 0.0f64;
        let mut c = 0.0f64;
        let mut n = 0.0f64;
        for i in 0..self.cells() {
            p += self.plant[i] as f64;
            c += self.carrion[i] as f64;
            n += self.nutrient[i] as f64;
        }
        (p, c, n)
    }

    /// Шаг среды: разложение падали, рост растений, диффузия питательных веществ.
    /// Каждая операция только перекладывает массу между пулами.
    pub fn update(&mut self, r: &ResourceConfig) {
        let n = self.cells();

        for i in 0..n {
            let d = self.carrion[i] * r.carrion_decay;
            self.carrion[i] -= d;
            self.nutrient[i] += d;
        }

        for i in 0..n {
            let p = self.plant[i];
            let growth = r.growth_rate * (p + r.plant_seed) * (1.0 - p / r.plant_capacity);
            if growth > 0.0 {
                let taken = growth.min(self.nutrient[i]);
                self.plant[i] = p + taken;
                self.nutrient[i] -= taken;
            } else if growth < 0.0 {
                // Выше потолка: лишняя масса отмирает обратно в питательные вещества.
                let back = (-growth).min(p);
                self.plant[i] = p - back;
                self.nutrient[i] += back;
            }
        }

        if r.nutrient_diffusion > 0.0 {
            self.diffuse_nutrient(r.nutrient_diffusion);
        }
    }

    fn diffuse_nutrient(&mut self, d: f32) {
        self.diff_buf.copy_from_slice(&self.nutrient);
        let keep = 1.0 - 4.0 * d;
        for y in 0..self.h {
            let yu = (if y == 0 { self.h - 1 } else { y - 1 }) * self.w;
            let yd = (if y + 1 == self.h { 0 } else { y + 1 }) * self.w;
            let row = y * self.w;
            for x in 0..self.w {
                let xl = if x == 0 { self.w - 1 } else { x - 1 };
                let xr = if x + 1 == self.w { 0 } else { x + 1 };
                let sum = self.diff_buf[yu + x]
                    + self.diff_buf[yd + x]
                    + self.diff_buf[row + xl]
                    + self.diff_buf[row + xr];
                self.nutrient[row + x] = self.diff_buf[row + x] * keep + d * sum;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_wrap_wraps_on_torus() {
        let w = World::new(4, 4);
        assert_eq!(w.step_wrap(0, 0, 0), (0, 3));
        assert_eq!(w.step_wrap(0, 0, 2), (3, 0));
        assert_eq!(w.step_wrap(3, 3, 1), (3, 0));
        assert_eq!(w.step_wrap(3, 3, 3), (0, 3));
    }

    #[test]
    fn update_keeps_cell_mass() {
        let mut w = World::new(16, 16);
        for i in 0..w.cells() {
            w.plant[i] = 0.5;
            w.carrion[i] = 0.2;
            w.nutrient[i] = 1.3;
        }
        let before = w.cell_mass();
        let cfg = ResourceConfig::default();
        for _ in 0..200 {
            w.update(&cfg);
        }
        let after = w.cell_mass();
        assert!(
            (after - before).abs() / before < 1e-4,
            "масса изменилась: было {before}, стало {after}"
        );
    }
}
