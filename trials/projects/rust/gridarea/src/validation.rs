#[cfg(test)]
mod tests {
    use crate::code::*;

    #[test]
    fn test_area() {
        let grid = Grid::new(vec![
            vec![true, false, true],
            vec![false, true, false],
            vec![true, false, true],
        ]);

        // Test border 0 (1x1 region)
        let area = grid.area(1, 1, 0);
        assert_eq!(area, vec![vec![true]]);

        // Test border 1 (3x3 region)
        let area = grid.area(1, 1, 1);
        assert_eq!(
            area,
            vec![
                vec![true, false, true],
                vec![false, true, false],
                vec![true, false, true],
            ]
        );

        // Test wrapping at edges
        let area = grid.area(2, 2, 1);
        assert_eq!(
            area,
            vec![
                vec![true, false, false],
                vec![false, true, true],
                vec![false, true, true],
            ]
        );
    }
}
