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

/// Window top-left, hugging the edge of `work` and never past its ends, so the pill (`pill` size) sits at `p`;
/// plus the pill's centre along the edge, measured from the window's start (`anchor`). Near a corner the window
/// stops at the screen edge and the anchor moves off its middle, so the open island can be kept on screen too.
pub fn window_pos(p: Placement, work: (f64, f64, f64, f64), win: (f64, f64), pill: (f64, f64)) -> (f64, f64, f64) {
    let (x, y, w, h) = work;
    // pill centre on the screen, then the window around it, both kept inside [start, start + len]
    let place = |start: f64, len: f64, size: f64, win_len: f64| {
        let c = (start + p.pos * len).clamp(start + size / 2.0, start + len - size / 2.0);
        let ws = (c - win_len / 2.0).clamp(start, (start + len - win_len).max(start));
        (ws, c - ws)
    };
    match p.edge {
        Edge::Top | Edge::Bottom => {
            let (wx, anchor) = place(x, w, pill.0, win.0);
            (wx, if p.edge == Edge::Top { y } else { y + h - win.1 }, anchor)
        }
        Edge::Left | Edge::Right => {
            let (wy, anchor) = place(y, h, pill.1, win.1);
            (if p.edge == Edge::Left { x } else { x + w - win.0 }, wy, anchor)
        }
    }
}

/// While dragging: the window's top-left (physical px) that keeps `anchor` (window CSS px, the pill centre)
/// under the `cursor`, at the scale of the monitor the window is on now (it changes crossing monitors).
pub fn drag_window_pos(cursor: (f64, f64), anchor: (f64, f64), sf: f64) -> (i32, i32) {
    ((cursor.0 - anchor.0 * sf).round() as i32, (cursor.1 - anchor.1 * sf).round() as i32)
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
        let (_, y, _) = window_pos(Placement { edge: Edge::Left, pos: 0.0 }, WORK, win, pill);
        assert!(y + (win.1 - pill.1) / 2.0 >= 0.0, "pill top on screen");
        let (x, y, _) = window_pos(Placement { edge: Edge::Right, pos: 1.0 }, WORK, win, pill);
        assert!(y + (win.1 + pill.1) / 2.0 <= 1040.0, "pill bottom on screen");
        assert_eq!(x, 1920.0 - 1200.0);
        let (x, y, _) = window_pos(Placement { edge: Edge::Top, pos: 0.5 }, WORK, (1200.0, 700.0), (180.0, 40.0));
        assert_eq!((x, y), (360.0, 0.0)); // today's top-centre
        let (_, y, _) = window_pos(Placement { edge: Edge::Bottom, pos: 0.5 }, WORK, (1200.0, 700.0), (180.0, 40.0));
        assert_eq!(y, 340.0);
    }

    #[test]
    fn drag_keeps_the_anchor_under_the_pointer_at_each_scale() {
        // the pill centre (300, 30 CSS px) stays under the cursor: at 100% ...
        assert_eq!(drag_window_pos((1000.0, 500.0), (300.0, 30.0), 1.0), (700, 470));
        // ... and on a 150% monitor the same CSS point is 1.5x further from the window corner
        assert_eq!(drag_window_pos((3000.0, 500.0), (300.0, 30.0), 1.5), (2550, 455));
    }

    #[test]
    fn window_stays_in_work_area_and_reports_the_pill_anchor() {
        let (win, pill) = ((1200.0, 700.0), (180.0, 40.0));
        // near the top-left corner: the window starts at the screen edge, the pill sits 96 px into it
        let (x, y, anchor) = window_pos(Placement { edge: Edge::Top, pos: 0.05 }, WORK, win, pill);
        assert_eq!((x, y, anchor), (0.0, 0.0, 96.0));
        // near the top-right corner: mirrored
        let (x, _, anchor) = window_pos(Placement { edge: Edge::Top, pos: 0.95 }, WORK, win, pill);
        assert_eq!((x, anchor), (720.0, 1824.0 - 720.0));
        // centred: the anchor is the window's middle, as before
        assert_eq!(window_pos(Placement { edge: Edge::Top, pos: 0.5 }, WORK, win, pill).2, 600.0);
        // a side tab near the top: the window starts at the work area's top
        let (_, y, anchor) = window_pos(Placement { edge: Edge::Left, pos: 0.0 }, WORK, win, (40.0, 120.0));
        assert_eq!((y, anchor), (0.0, 60.0));
    }
}
