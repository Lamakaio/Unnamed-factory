//! Uniform spatial hash grid of rectangles, used to find the river segments and terrain edits
//! affecting a point or an area.

use bevy::{platform::collections::HashMap, prelude::*};

#[derive(Clone)]
pub struct SpatialGrid {
    cell_size: f32,
    /// Ids of the items overlapping each cell, in insertion order.
    cells: HashMap<IVec2, Vec<u32>>,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            cells: HashMap::default(),
        }
    }

    fn cell(&self, p: Vec2) -> IVec2 {
        (p / self.cell_size).floor().as_ivec2()
    }

    /// Register item `id`, covering the rectangle `min..max`.
    pub fn insert(&mut self, id: u32, min: Vec2, max: Vec2) {
        let (min, max) = (self.cell(min), self.cell(max));
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                self.cells.entry(IVec2::new(x, y)).or_default().push(id);
            }
        }
    }

    /// Items whose rectangle may contain `p`, in insertion order.
    pub fn query_point(&self, p: Vec2) -> &[u32] {
        self.cells
            .get(&self.cell(p))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Items whose rectangle may overlap `min..max`. Ids may repeat.
    pub fn query_rect(&self, min: Vec2, max: Vec2) -> impl Iterator<Item = u32> + '_ {
        let (min, max) = (self.cell(min), self.cell(max));
        (min.x..=max.x)
            .flat_map(move |x| (min.y..=max.y).map(move |y| IVec2::new(x, y)))
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
            .copied()
    }
}

/// Whether rectangles `a_min..a_max` and `b_min..b_max` overlap.
pub fn rects_overlap(a_min: Vec2, a_max: Vec2, b_min: Vec2, b_max: Vec2) -> bool {
    a_min.cmple(b_max).all() && b_min.cmple(a_max).all()
}
