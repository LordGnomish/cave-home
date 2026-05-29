//! Plain geometry the camera brain needs: a point, an axis-aligned bounding
//! box, a detection polygon, a robust point-in-polygon test and
//! intersection-over-union (`IoU`).
//!
//! Everything here works in the camera's normalised picture space: `x` runs
//! left→right, `y` runs top→bottom, both as plain `f64`. Nothing here touches a
//! pixel buffer, a codec or a model — it is pure coordinate arithmetic, so it is
//! cheap to test against hand-worked values.

/// A point in the picture, left→right `x`, top→bottom `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Horizontal position (left = 0).
    pub x: f64,
    /// Vertical position (top = 0).
    pub y: f64,
}

impl Point {
    /// A point at `(x, y)`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned bounding box: top-left corner `(x, y)` plus `width` and
/// `height`. This is the shape a detector draws around a thing it found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width (kept ≥ 0 by [`BBox::new`]).
    pub width: f64,
    /// Height (kept ≥ 0 by [`BBox::new`]).
    pub height: f64,
}

impl BBox {
    /// A box at `(x, y)` with the given `width` and `height`.
    ///
    /// A negative width or height would describe an inside-out box and break
    /// every area / overlap calculation downstream, so it is clamped to zero
    /// here rather than left to surprise a later step.
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    /// Right edge (`x + width`).
    #[must_use]
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge (`y + height`).
    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Area of the box.
    #[must_use]
    pub fn area(&self) -> f64 {
        self.width * self.height
    }

    /// The geometric centre of the box.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// The middle of the box's bottom edge. For a standing person or a car this
    /// is roughly where it touches the ground, which is the point a household
    /// actually means by "in the driveway" — so it is the natural anchor for
    /// zone membership.
    #[must_use]
    pub fn bottom_center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.bottom())
    }

    /// The overlapping area of two boxes (zero if they do not overlap).
    #[must_use]
    pub fn intersection_area(&self, other: &Self) -> f64 {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        let w = (right - left).max(0.0);
        let h = (bottom - top).max(0.0);
        w * h
    }
}

/// Intersection-over-union of two boxes: the overlap area divided by the area
/// they jointly cover. `1.0` means identical boxes; `0.0` means no overlap.
///
/// `IoU` is how the tracker decides whether a box in this frame is the same thing
/// as a box in the last frame. Two zero-area boxes (or a pair that does not
/// overlap) return `0.0` rather than dividing by zero.
#[must_use]
pub fn iou(a: &BBox, b: &BBox) -> f64 {
    let inter = a.intersection_area(b);
    let union = a.area() + b.area() - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// A closed polygon describing a detection zone — an ordered ring of vertices.
/// The last vertex is implicitly joined back to the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    points: Vec<Point>,
}

impl Polygon {
    /// Build a polygon from an ordered list of vertices. A polygon needs at
    /// least three points to enclose any area; fewer is rejected so a
    /// degenerate "zone" can never silently swallow (or reject) every
    /// detection.
    ///
    /// # Errors
    /// [`PolygonError::TooFewPoints`] if fewer than three vertices are given.
    pub fn new(points: Vec<Point>) -> Result<Self, PolygonError> {
        if points.len() < 3 {
            Err(PolygonError::TooFewPoints)
        } else {
            Ok(Self { points })
        }
    }

    /// The polygon's vertices, in order.
    #[must_use]
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// Whether `p` lies inside (or on the boundary of) the polygon.
    ///
    /// This is the standard ray-casting (even-odd) test: count how many times a
    /// ray cast to the right from `p` crosses the polygon's edges; an odd count
    /// means inside. Two cases that the naive version gets wrong are handled
    /// explicitly so a detection sitting exactly on a drawn boundary line is
    /// not flickered in and out of the zone frame to frame:
    ///
    /// - a point lying *on* an edge (including a vertex) is reported as inside;
    /// - horizontal edges and the "ray grazes a vertex" case use a half-open
    ///   crossing rule (`y_i > p.y` XOR `y_j > p.y`) so each crossing is counted
    ///   exactly once.
    #[must_use]
    pub fn contains(&self, p: Point) -> bool {
        let pts = &self.points;
        let n = pts.len();
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let pi = pts[i];
            let pj = pts[j];
            // On-boundary check first: exactly on an edge counts as inside.
            if point_on_segment(p, pi, pj) {
                return true;
            }
            // Half-open crossing rule, robust to vertices and horizontal edges.
            let crosses = (pi.y > p.y) != (pj.y > p.y);
            if crosses {
                let x_at_p = (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x;
                if p.x < x_at_p {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }
}

/// Why a [`Polygon`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolygonError {
    /// Fewer than three vertices were supplied.
    TooFewPoints,
}

impl core::fmt::Display for PolygonError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFewPoints => f.write_str("a detection zone needs at least three corners"),
        }
    }
}

impl std::error::Error for PolygonError {}

/// Whether `p` lies on the segment `a`–`b` (within a small tolerance), used by
/// [`Polygon::contains`] to treat boundary points as inside.
fn point_on_segment(p: Point, a: Point, b: Point) -> bool {
    const EPS: f64 = 1e-9;
    // Cross product near zero => collinear with the segment's line.
    let cross = (b.x - a.x).mul_add(p.y - a.y, -((b.y - a.y) * (p.x - a.x)));
    if cross.abs() > EPS {
        return false;
    }
    // Within the segment's bounding box (collinear is not enough on its own).
    let within_x = p.x >= a.x.min(b.x) - EPS && p.x <= a.x.max(b.x) + EPS;
    let within_y = p.y >= a.y.min(b.y) - EPS && p.y <= a.y.max(b.y) + EPS;
    within_x && within_y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Polygon {
        // Unit square (0,0)-(10,0)-(10,10)-(0,10).
        Polygon::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ])
        .expect("valid square")
    }

    fn l_shape() -> Polygon {
        // A concave L: the notch is the upper-right quadrant.
        Polygon::new(vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 4.0),
            Point::new(4.0, 4.0),
            Point::new(4.0, 10.0),
            Point::new(0.0, 10.0),
        ])
        .expect("valid L")
    }

    #[test]
    fn polygon_rejects_fewer_than_three_points() {
        assert_eq!(
            Polygon::new(vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
            Err(PolygonError::TooFewPoints)
        );
    }

    #[test]
    fn convex_inside_point() {
        assert!(square().contains(Point::new(5.0, 5.0)));
    }

    #[test]
    fn convex_outside_point() {
        assert!(!square().contains(Point::new(15.0, 5.0)));
        assert!(!square().contains(Point::new(-1.0, 5.0)));
        assert!(!square().contains(Point::new(5.0, 20.0)));
    }

    #[test]
    fn boundary_edge_point_is_inside() {
        // Exactly on the bottom edge.
        assert!(square().contains(Point::new(5.0, 0.0)));
        // Exactly on the right edge.
        assert!(square().contains(Point::new(10.0, 5.0)));
    }

    #[test]
    fn vertex_point_is_inside() {
        assert!(square().contains(Point::new(0.0, 0.0)));
        assert!(square().contains(Point::new(10.0, 10.0)));
    }

    #[test]
    fn concave_point_in_solid_part_is_inside() {
        // Lower-left arm of the L.
        assert!(l_shape().contains(Point::new(2.0, 8.0)));
        assert!(l_shape().contains(Point::new(8.0, 2.0)));
    }

    #[test]
    fn concave_point_in_the_notch_is_outside() {
        // Upper-right quadrant is the cut-out of the L.
        assert!(!l_shape().contains(Point::new(8.0, 8.0)));
    }

    #[test]
    fn bbox_clamps_negative_dimensions() {
        let b = BBox::new(5.0, 5.0, -3.0, -4.0);
        assert_eq!(b.width, 0.0);
        assert_eq!(b.height, 0.0);
        assert_eq!(b.area(), 0.0);
    }

    #[test]
    fn bbox_center_and_bottom_center() {
        let b = BBox::new(0.0, 0.0, 10.0, 20.0);
        assert_eq!(b.center(), Point::new(5.0, 10.0));
        assert_eq!(b.bottom_center(), Point::new(5.0, 20.0));
    }

    #[test]
    fn iou_identical_boxes_is_one() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        assert!((iou(&a, &a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn iou_disjoint_boxes_is_zero() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(100.0, 100.0, 10.0, 10.0);
        assert_eq!(iou(&a, &b), 0.0);
    }

    #[test]
    fn iou_half_overlap_known_value() {
        // Two 10x10 boxes shifted 5 in x: intersection 5x10=50, union
        // 100+100-50=150 -> 1/3.
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(5.0, 0.0, 10.0, 10.0);
        assert!((iou(&a, &b) - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn iou_quarter_corner_overlap_known_value() {
        // Overlap is a 5x5=25 corner; union 100+100-25=175 -> 25/175 = 1/7.
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(5.0, 5.0, 10.0, 10.0);
        assert!((iou(&a, &b) - (1.0 / 7.0)).abs() < 1e-9);
    }

    #[test]
    fn iou_contained_box_known_value() {
        // 5x5 fully inside 10x10: inter 25, union 100 -> 0.25.
        let outer = BBox::new(0.0, 0.0, 10.0, 10.0);
        let inner = BBox::new(2.0, 2.0, 5.0, 5.0);
        assert!((iou(&outer, &inner) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn iou_zero_area_boxes_dont_divide_by_zero() {
        let a = BBox::new(0.0, 0.0, 0.0, 0.0);
        let b = BBox::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(iou(&a, &b), 0.0);
    }
}
