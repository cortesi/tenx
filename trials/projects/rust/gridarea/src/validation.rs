#[cfg(test)]
mod tests {
    use crate::code::*;

    fn tarea<T: Area>(instance: &T) {
        // Test border 0 (1x1 region)
        let area = instance.area(1, 1, 0);
        assert_eq!(area, vec![vec![false]]);

        // Test border 1 (3x3 region)
        let area = instance.area(1, 1, 1);
        assert_eq!(
            area,
            vec![
                vec![true, false, true],
                vec![true, false, false],
                vec![false, true, true],
            ]
        );

        // Test wrapping at edges
        let area = instance.area(2, 2, 1);
        assert_eq!(
            area,
            vec![
                vec![false, false, true],
                vec![true, true, false],
                vec![false, true, true],
            ]
        );

        // Test corner top-left
        let area_top_left = instance.area(0, 0, 1);
        assert_eq!(
            area_top_left,
            vec![
                vec![true, false, true],
                vec![true, true, false],
                vec![false, true, false],
            ]
        );

        // Test corner bottom-right
        let area_bottom_right = instance.area(2, 2, 1);
        assert_eq!(
            area_bottom_right,
            vec![
                vec![false, false, true],
                vec![true, true, false],
                vec![false, true, true],
            ]
        );

        // Test border 2 (5x5 region), focusing on wrapping
        let area_wrapping = instance.area(0, 0, 2);
        assert_eq!(
            area_wrapping,
            vec![
                vec![false, false, true, false, false],
                vec![true, true, false, true, true],
                vec![false, true, true, false, true],
                vec![false, false, true, false, false],
                vec![true, true, false, true, true],
            ]
        );
    }

    #[test]
    fn test_grid_area() {
        let grid = Grid::new(vec![
            vec![true, false, true],
            vec![true, false, false],
            vec![false, true, true],
        ]);
        tarea(&grid);
    }
    #[test]

    fn test_sparse_area() {
        let s = Sparse::new(vec![
            vec![true, false, true],
            vec![true, false, false],
            vec![false, true, true],
        ]);
        tarea(&s);
    }
}
