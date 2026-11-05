//! Детерминированный генератор случайных чисел (xoshiro256++).
//!
//! Своя реализация вместо `rand`: требование проекта — побитово
//! воспроизводимые прогоны, не зависящие от версии зависимостей
//! и от платформы.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 для разворачивания одного сида в состояние из четырёх слов
        let mut z = seed;
        let mut s = [0u64; 4];
        for slot in s.iter_mut() {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *slot = x ^ (x >> 31);
        }
        Self { s }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[0]
            .wrapping_add(self.s[3])
            .rotate_left(23)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Равномерное число в [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) * (1.0 / 16_777_216.0)
    }

    /// Равномерное целое в [0, n). Требует n > 0.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        (((self.next_u32() as u64) * (n as u64)) >> 32) as u32
    }

    /// Заполняет срез гауссовым шумом со стандартным отклонением `sigma`.
    pub fn fill_gaussian(&mut self, out: &mut [f32], sigma: f32) {
        let mut i = 0;
        while i < out.len() {
            let (a, b) = self.gaussian_pair();
            out[i] = a * sigma;
            i += 1;
            if i < out.len() {
                out[i] = b * sigma;
                i += 1;
            }
        }
    }

    /// Добавляет гауссов шум к срезу — основная операция мутации весов.
    pub fn add_gaussian(&mut self, out: &mut [f32], sigma: f32) {
        let mut i = 0;
        while i < out.len() {
            let (a, b) = self.gaussian_pair();
            out[i] += a * sigma;
            i += 1;
            if i < out.len() {
                out[i] += b * sigma;
                i += 1;
            }
        }
    }

    /// Полярный метод Марсальи: пара независимых N(0, 1) за один заход.
    #[inline]
    fn gaussian_pair(&mut self) -> (f32, f32) {
        loop {
            let u = self.next_f32() * 2.0 - 1.0;
            let v = self.next_f32() * 2.0 - 1.0;
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                let f = (-2.0 * s.ln() / s).sqrt();
                return (u * f, v * f);
            }
        }
    }
}
