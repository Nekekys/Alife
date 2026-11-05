//! Хранилище особей: структура из массивов (SoA) фиксированной ёмкости.
//!
//! Все поля — плоские массивы длиной `capacity`, геномы лежат одним
//! непрерывным блоком. Такая раскладка нужна и для скорости на CPU,
//! и для того, чтобы позже отдать те же буферы в compute-шейдер.

#[derive(Debug)]
pub struct Agents {
    pub capacity: usize,
    pub genome_len: usize,
    pub alive: Vec<bool>,
    pub x: Vec<u16>,
    pub y: Vec<u16>,
    pub energy: Vec<f32>,
    pub age: Vec<u32>,
    /// Сквозной номер особи за всё время прогона.
    pub id: Vec<u64>,
    pub parent: Vec<u64>,
    /// Номер корневого предка: грубая метка линии до появления филогении.
    pub lineage: Vec<u64>,
    pub generation: Vec<u32>,
    /// Ела ли особь на прошлом шаге — вход сенсора.
    pub last_ate: Vec<f32>,
    /// Била ли её кто-то на прошлом шаге — вход сенсора.
    pub last_hit: Vec<f32>,
    pub genomes: Vec<f32>,
    free: Vec<u32>,
    live: usize,
    next_id: u64,
}

impl Agents {
    pub fn new(capacity: usize, genome_len: usize) -> Self {
        let mut free: Vec<u32> = Vec::with_capacity(capacity);
        // В обратном порядке: слоты раздаются по возрастанию индекса.
        for i in (0..capacity).rev() {
            free.push(i as u32);
        }
        Self {
            capacity,
            genome_len,
            alive: vec![false; capacity],
            x: vec![0; capacity],
            y: vec![0; capacity],
            energy: vec![0.0; capacity],
            age: vec![0; capacity],
            id: vec![0; capacity],
            parent: vec![0; capacity],
            lineage: vec![0; capacity],
            generation: vec![0; capacity],
            last_ate: vec![0.0; capacity],
            last_hit: vec![0.0; capacity],
            genomes: vec![0.0; capacity * genome_len],
            free,
            live: 0,
            next_id: 1,
        }
    }

    #[inline]
    pub fn live(&self) -> usize {
        self.live
    }

    #[inline]
    pub fn genome(&self, slot: usize) -> &[f32] {
        let o = slot * self.genome_len;
        &self.genomes[o..o + self.genome_len]
    }

    #[inline]
    pub fn genome_mut(&mut self, slot: usize) -> &mut [f32] {
        let o = slot * self.genome_len;
        &mut self.genomes[o..o + self.genome_len]
    }

    pub fn copy_genome(&mut self, src: usize, dst: usize) {
        let gl = self.genome_len;
        let s = src * gl;
        self.genomes.copy_within(s..s + gl, dst * gl);
    }

    /// Занимает свободный слот. Возвращает `None`, когда ёмкость исчерпана.
    pub fn alloc(&mut self) -> Option<usize> {
        self.free.pop().map(|s| s as usize)
    }

    /// Заполняет слот новой особью и возвращает её номер.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        slot: usize,
        x: u16,
        y: u16,
        energy: f32,
        parent_id: u64,
        lineage: u64,
        generation: u32,
    ) -> u64 {
        self.alive[slot] = true;
        self.x[slot] = x;
        self.y[slot] = y;
        self.energy[slot] = energy;
        self.age[slot] = 0;
        self.last_ate[slot] = 0.0;
        self.last_hit[slot] = 0.0;
        let id = self.next_id;
        self.next_id += 1;
        self.id[slot] = id;
        self.parent[slot] = parent_id;
        // lineage == 0 означает «основатель»: линией становится собственный номер.
        self.lineage[slot] = if lineage == 0 { id } else { lineage };
        self.generation[slot] = generation;
        self.live += 1;
        id
    }

    /// Освобождает слот. Занятость клетки снимает вызывающая сторона.
    pub fn release(&mut self, slot: usize) {
        debug_assert!(self.alive[slot]);
        self.alive[slot] = false;
        self.energy[slot] = 0.0;
        self.free.push(slot as u32);
        self.live -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_reused_in_order() {
        let mut a = Agents::new(4, 2);
        let s0 = a.alloc().unwrap();
        let s1 = a.alloc().unwrap();
        assert_eq!((s0, s1), (0, 1));
        a.spawn(s0, 0, 0, 1.0, 0, 0, 0);
        a.spawn(s1, 1, 0, 1.0, 0, 0, 0);
        assert_eq!(a.live(), 2);
        a.release(s0);
        assert_eq!(a.live(), 1);
        assert_eq!(a.alloc().unwrap(), 0);
    }

    #[test]
    fn capacity_runs_out() {
        let mut a = Agents::new(2, 1);
        assert!(a.alloc().is_some());
        assert!(a.alloc().is_some());
        assert!(a.alloc().is_none());
    }

    #[test]
    fn copy_genome_duplicates_weights() {
        let mut a = Agents::new(2, 3);
        a.genome_mut(0).copy_from_slice(&[1.0, 2.0, 3.0]);
        a.copy_genome(0, 1);
        assert_eq!(a.genome(1), &[1.0, 2.0, 3.0]);
    }
}
