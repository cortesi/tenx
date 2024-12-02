pub trait Surrounding {
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

impl Surrounding for Grid {
    fn area(&self, x: usize, y: usize, border: usize) -> Vec<Vec<bool>> {
        let height = self.cells.len();
        let width = self.cells[0].len();
        let size = 2 * border + 1;
        let mut result = vec![vec![false; size]; size];
        (0..size).for_each(|dy| {
            for dx in 0..size {
                let mut nx = x as i32 + (dx as i32 - border as i32);
                if nx < 0 {
                    nx = width as i32 - (-nx) % width as i32;
                }
                nx %= width as i32;

                let ny = (y as i32 + dy as i32 - border as i32).rem_euclid(height as i32);

                result[dy][dx] = self.cells[ny as usize][nx as usize];
            }
        });
        result
    }
}

impl Grid {
    pub fn new(cells: Vec<Vec<bool>>) -> Self {
        Self { cells }
    }
}
