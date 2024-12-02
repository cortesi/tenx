pub trait Area {
    /// Get the surrounding region for a cell. The border specifies the number of surrounding
    /// cells to include. If border is 0, we return a 1x1 region, if it is 1, we return a 3x3
    /// region, etc. The regions are always centered around the specified cell.
    ///
    /// The grid is treated as a Moebius strip, so the left edge is connected to the right edge.
    fn area(&self, x: usize, y: usize, border: usize) -> Vec<Vec<bool>>;
}

pub struct Grid {
    pub cells: Vec<Vec<bool>>,
}

impl Grid {
    pub fn new(cells: Vec<Vec<bool>>) -> Self {
        Self { cells }
    }
}

impl Area for Grid {
    fn area(&self, x: usize, y: usize, border: usize) -> Vec<Vec<bool>> {
        unimplemented!()
    }
}

/// A sparse grid. This is a grid where most cells are empty, and we only store the cells that are
/// on.
pub struct Sparse {
    pub map: std::collections::HashMap<(usize, usize), bool>,
}

impl Sparse {
    pub fn new(cells: Vec<Vec<bool>>) -> Self {
        let mut map = std::collections::HashMap::new();
        for (x, row) in cells.iter().enumerate() {
            for (y, &cell) in row.iter().enumerate() {
                if cell {
                    map.insert((x, y), cell);
                }
            }
        }
        Self { map }
    }
}

impl Area for Sparse {
    fn area(&self, x: usize, y: usize, border: usize) -> Vec<Vec<bool>> {
        unimplemented!()
    }
}
