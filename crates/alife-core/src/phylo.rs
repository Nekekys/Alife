//! Филогения: кто от кого произошёл и когда линии разошлись.
//!
//! Дерево хранит только предков ныне живущих. Ветвь, у которой не осталось
//! живых потомков, удаляется сразу, иначе за сотню тысяч шагов накопились бы
//! миллионы мёртвых узлов. По той же причине узлы лежат в арене и ссылаются
//! на родителя по индексу: подъём по дереву обходится без хеш-таблицы.

use std::collections::HashMap;

const NO_NODE: u32 = u32::MAX;

#[derive(Debug, Clone)]
struct Node {
    id: u64,
    parent: u32,
    birth_step: u64,
    generation: u32,
    /// Сколько живых особей в поддереве этого узла, включая его самого.
    living_below: u32,
    /// Жива ли особь, которой этот узел принадлежит.
    alive: bool,
}

#[derive(Debug, Default)]
pub struct Phylogeny {
    nodes: Vec<Node>,
    index: HashMap<u64, u32>,
    free: Vec<u32>,
    living: u32,
}

/// Сводка по дереву на один замер.
#[derive(Debug, Clone, Default)]
pub struct PhyloStats {
    /// Размер отсечённого дерева: сколько предков живых особей хранится.
    pub nodes: usize,
    /// Сколько линий-основателей дожило до сейчас.
    pub roots: usize,
    /// Поколение последнего общего предка всех живых.
    /// Имеет смысл только при одной выжившей линии.
    pub mrca_generation: u32,
    /// Шаг, на котором этот предок родился.
    pub mrca_step: u64,
    /// Максимальное поколение среди живых.
    pub max_generation: u32,
}

impl Phylogeny {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn living(&self) -> u32 {
        self.living
    }

    pub fn size(&self) -> usize {
        self.index.len()
    }

    /// Регистрирует рождение. `parent_id` равен нулю у особей-основателей.
    pub fn birth(&mut self, id: u64, parent_id: u64, step: u64, generation: u32) {
        let parent = if parent_id == 0 {
            NO_NODE
        } else {
            self.index.get(&parent_id).copied().unwrap_or(NO_NODE)
        };
        let node = Node { id, parent, birth_step: step, generation, living_below: 1, alive: true };
        let slot = match self.free.pop() {
            Some(s) => {
                self.nodes[s as usize] = node;
                s
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            }
        };
        self.index.insert(id, slot);
        self.living += 1;

        let mut cur = parent;
        while cur != NO_NODE {
            self.nodes[cur as usize].living_below += 1;
            cur = self.nodes[cur as usize].parent;
        }
    }

    /// Регистрирует смерть и отсекает ветви, у которых не осталось живых.
    pub fn death(&mut self, id: u64) {
        let Some(&slot) = self.index.get(&id) else {
            return;
        };
        self.nodes[slot as usize].alive = false;
        self.living -= 1;

        let mut cur = slot;
        while cur != NO_NODE {
            self.nodes[cur as usize].living_below -= 1;
            cur = self.nodes[cur as usize].parent;
        }

        // Снизу вверх убираем всё, под чем никого не осталось.
        let mut cur = slot;
        while cur != NO_NODE {
            let n = &self.nodes[cur as usize];
            if n.living_below > 0 || n.alive {
                break;
            }
            let parent = n.parent;
            let dead_id = n.id;
            self.index.remove(&dead_id);
            self.free.push(cur);
            cur = parent;
        }
    }

    /// `living_id` — номер любой живой особи; от неё ведётся подъём к общему предку.
    pub fn stats(&self, living_id: Option<u64>) -> PhyloStats {
        let mut roots = 0usize;
        let mut max_generation = 0u32;
        for (_, &slot) in self.index.iter() {
            let n = &self.nodes[slot as usize];
            if n.parent == NO_NODE {
                roots += 1;
            }
            if n.alive {
                max_generation = max_generation.max(n.generation);
            }
        }

        let mut mrca_generation = 0;
        let mut mrca_step = 0;
        if roots == 1 {
            if let Some(start) = living_id.and_then(|id| self.index.get(&id).copied()) {
                let mut cur = start;
                loop {
                    let n = &self.nodes[cur as usize];
                    // Первый снизу узел, под которым собрались все живые, и есть
                    // последний общий предок.
                    if n.living_below == self.living {
                        mrca_generation = n.generation;
                        mrca_step = n.birth_step;
                        break;
                    }
                    if n.parent == NO_NODE {
                        mrca_generation = n.generation;
                        mrca_step = n.birth_step;
                        break;
                    }
                    cur = n.parent;
                }
            }
        }

        PhyloStats {
            nodes: self.index.len(),
            roots,
            mrca_generation,
            mrca_step,
            max_generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_branches_are_pruned() {
        let mut p = Phylogeny::new();
        p.birth(1, 0, 0, 0);
        p.birth(2, 1, 10, 1);
        p.birth(3, 2, 20, 2);
        assert_eq!(p.size(), 3);
        assert_eq!(p.living(), 3);

        // Умирает самый глубокий — его узел уходит, предки остаются живыми.
        p.death(3);
        assert_eq!(p.size(), 2);
        p.death(2);
        assert_eq!(p.size(), 1);
        p.death(1);
        assert_eq!(p.size(), 0, "дерево без живых должно опустеть целиком");
        assert_eq!(p.living(), 0);
    }

    #[test]
    fn ancestors_survive_their_children() {
        let mut p = Phylogeny::new();
        p.birth(1, 0, 0, 0);
        p.birth(2, 1, 10, 1);
        // Родитель умер, но у него есть живой потомок: узел обязан остаться.
        p.death(1);
        assert_eq!(p.size(), 2);
        assert_eq!(p.living(), 1);
        p.death(2);
        assert_eq!(p.size(), 0);
    }

    #[test]
    fn mrca_of_siblings_is_their_parent() {
        let mut p = Phylogeny::new();
        p.birth(1, 0, 0, 0);
        p.birth(2, 1, 100, 1);
        p.birth(3, 1, 150, 1);
        p.death(1);

        let s = p.stats(Some(2));
        assert_eq!(s.roots, 1);
        assert_eq!(s.mrca_generation, 0, "общий предок двух братьев — их родитель");
        assert_eq!(s.mrca_step, 0);
        assert_eq!(s.max_generation, 1);
    }

    #[test]
    fn mrca_moves_down_when_a_branch_dies_out() {
        let mut p = Phylogeny::new();
        p.birth(1, 0, 0, 0); // основатель
        p.birth(2, 1, 100, 1); // ветвь A
        p.birth(3, 1, 100, 1); // ветвь B
        p.birth(4, 3, 200, 2);
        p.birth(5, 3, 200, 2);
        p.death(1);
        p.death(3);
        assert_eq!(p.stats(Some(4)).mrca_generation, 0, "пока жива ветвь A, предок — основатель");

        // Ветвь A вымерла: все живые теперь потомки узла 3.
        p.death(2);
        let s = p.stats(Some(4));
        assert_eq!(s.mrca_generation, 1);
        assert_eq!(s.mrca_step, 100);
    }

    #[test]
    fn separate_founders_count_as_separate_roots() {
        let mut p = Phylogeny::new();
        p.birth(1, 0, 0, 0);
        p.birth(2, 0, 0, 0);
        let s = p.stats(Some(1));
        assert_eq!(s.roots, 2);
        assert_eq!(s.mrca_generation, 0, "общего предка у разных основателей нет");
    }

    #[test]
    fn freed_slots_are_reused() {
        let mut p = Phylogeny::new();
        for id in 1..=100u64 {
            p.birth(id, 0, id, 0);
        }
        for id in 1..=100u64 {
            p.death(id);
        }
        assert_eq!(p.size(), 0);
        for id in 101..=200u64 {
            p.birth(id, 0, id, 0);
        }
        // Арена не должна расти, раз все прежние слоты освободились.
        assert_eq!(p.nodes.len(), 100, "слоты арены не переиспользуются");
    }
}
