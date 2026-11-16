//! Where the island sits: an edge of the work area and a position along it (0..1). Pure: `window::place`
//! turns it into a window position, `island_drag_end` makes one from where the pointer let go.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub edge: Edge,
    pub pos: f64,
}

impl Default for Placement {
    fn default() -> Self {
        Placement { edge: Edge::Top, pos: 0.5 }
    }
}

const NAMES: [(Edge, &str); 4] = [(Edge::Top, "top"), (Edge::Bottom, "bottom"), (Edge::Left, "left"), (Edge::Right, "right")];

/// `"left:0.25"`; anything unreadable is the default (top centre), `pos` is clamped to 0..1.
pub fn parse(s: &str) -> Placement {
    let Some((e, p)) = s.split_once(':') else { return Placement::default() };
    let (Some(edge), Ok(pos)) = (NAMES.iter().find(|(_, n)| *n == e).map(|(e, _)| *e), p.parse::<f64>()) else {
        return Placement::default();
    };
    if !pos.is_finite() {
        return Placement::default();
    }
    Placement { edge, pos: pos.clamp(0.0, 1.0) }
}

pub fn format(p: Placement) -> String {
    let name = NAMES.iter().find(|(e, _)| *e == p.edge).map_or("top", |(_, n)| n);
    format!("{name}:{}", (p.pos * 10_000.0).round() / 10_000.0)
}

/// The edge of `work` (x, y, w, h) nearest to the point, and the point's position along that edge.
pub fn snap(cx: f64, cy: f64, work: (f64, f64, f64, f64)) -> Placement {
    let (x, y, w, h) = work;
    let along_x = ((cx - x) / w).clamp(0.0, 1.0);
    let along_y = ((cy - y) / h).clamp(0.0, 1.0);
    let options = [(cy - y, Edge::Top, along_x), (y + h - cy, Edge::Bottom, along_x), (cx - x, Edge::Left, along_y), (x + w - cx, Edge::Right, along_y)];
    let (_, edge, pos) = options.into_iter().fold(options[0], |best, o| if o.0 < best.0 { o } else { best });
    Placement { edge, pos }
}

/// Window top-left so the pill (`pill` size, centred in the window across the edge) sits at `p`, hugging the
/// edge of `work`, never past its ends.
pub fn window_pos(p: Placement, work: (f64, f64, f64, f64), win: (f64, f64), pill: (f64, f64)) -> (f64, f64) {
    let (x, y, w, h) = work;
    let along = |start: f64, len: f64, size: f64| (start + p.pos * len).clamp(start + size / 2.0, start + len - size / 2.0);
    match p.edge {
        Edge::Top | Edge::Bottom => {
            let wx = along(x, w, pill.0) - win.0 / 2.0;
            (wx, if p.edge == Edge::Top { y } else { y + h - win.1 })
        }
        Edge::Left | Edge::Right => {
            let wy = along(y, h, pill.1) - win.1 / 2.0;
            (if p.edge == Edge::Left { x } else { x + w - win.0 }, wy)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: (f64, f64, f64, f64) = (0.0, 0.0, 1920.0, 1040.0);

    #[test]
    fn parse_round_trips_and_defaults() {
        assert_eq!(format(parse("left:0.25")), "left:0.25");
        assert_eq!(parse("nope"), Placement { edge: Edge::Top, pos: 0.5 });
        assert_eq!(parse("top:7"), Placement { edge: Edge::Top, pos: 1.0 });
    }

    #[test]
    fn snap_picks_nearest_edge() {
        assert_eq!(snap(960.0, 3.0, WORK), Placement { edge: Edge::Top, pos: 0.5 });
        assert_eq!(snap(5.0, 520.0, WORK), Placement { edge: Edge::Left, pos: 0.5 });
        assert_eq!(snap(1915.0, 260.0, WORK), Placement { edge: Edge::Right, pos: 0.25 });
        assert_eq!(snap(960.0, 1035.0, WORK), Placement { edge: Edge::Bottom, pos: 0.5 });
    }

    #[test]
    fn corner_release_goes_to_nearer_edge() {
        let p = snap(10.0, 30.0, WORK);
        assert_eq!(p.edge, Edge::Left);
        assert!((p.pos - 30.0 / 1040.0).abs() < 1e-9);
    }

    #[test]
    fn window_keeps_pill_on_screen() {
        let (win, pill) = ((1200.0, 700.0), (40.0, 120.0));
        let (_, y) = window_pos(Placement { edge: Edge::Left, pos: 0.0 }, WORK, win, pill);
        assert!(y + (win.1 - pill.1) / 2.0 >= 0.0, "pill top on screen");
        let (x, y) = window_pos(Placement { edge: Edge::Right, pos: 1.0 }, WORK, win, pill);
        assert!(y + (win.1 + pill.1) / 2.0 <= 1040.0, "pill bottom on screen");
        assert_eq!(x, 1920.0 - 1200.0);
        let (x, y) = window_pos(Placement { edge: Edge::Top, pos: 0.5 }, WORK, (1200.0, 700.0), (180.0, 40.0));
        assert_eq!((x, y), (360.0, 0.0)); // today's top-centre
        let (_, y) = window_pos(Placement { edge: Edge::Bottom, pos: 0.5 }, WORK, (1200.0, 700.0), (180.0, 40.0));
        assert_eq!(y, 340.0);
    }
}
