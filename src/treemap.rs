use eframe::egui::{Rect, pos2};

use crate::scan::Tree;

pub const HEADER_HEIGHT: f32 = 16.0;
const PADDING: f32 = 2.0;
/// Boxes smaller than this (in either dimension) are not drawn or subdivided.
const MIN_SIDE: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    Dir,
    /// The loose files directly inside a directory, lumped into one box.
    Files,
}

pub struct LaidOutBox {
    pub rect: Rect,
    pub dir: usize,
    pub kind: BoxKind,
    pub depth: usize,
    /// Hue inherited from the top-level branch this box belongs to.
    pub hue: f32,
    pub has_header: bool,
}

/// Lays out `view` and all its descendants inside `rect`. Parents come before their children,
/// so painting in order draws nested boxes on top, and hit-testing in reverse finds the deepest.
pub fn layout(tree: &Tree, view: usize, rect: Rect) -> Vec<LaidOutBox> {
    let mut out = Vec::new();
    layout_dir(tree, view, rect, 0, 0.0, &mut out);
    out
}

fn layout_dir(tree: &Tree, id: usize, rect: Rect, depth: usize, hue: f32, out: &mut Vec<LaidOutBox>) {
    if rect.width() < MIN_SIDE || rect.height() < MIN_SIDE {
        return;
    }
    let has_header = rect.height() > HEADER_HEIGHT * 2.0 && rect.width() > 40.0;
    out.push(LaidOutBox { rect, dir: id, kind: BoxKind::Dir, depth, hue, has_header });

    let top = if has_header { rect.min.y + HEADER_HEIGHT } else { rect.min.y + PADDING };
    let inner = Rect::from_min_max(pos2(rect.min.x + PADDING, top), rect.max - eframe::egui::vec2(PADDING, PADDING));
    if inner.width() < MIN_SIDE || inner.height() < MIN_SIDE {
        return;
    }

    let dir = &tree.dirs[id];
    // Children are already sorted by size; the loose-files box is slotted in at its rank.
    let mut items: Vec<(u64, Option<usize>)> =
        dir.children.iter().map(|&c| (tree.dirs[c].size, Some(c))).filter(|(s, _)| *s > 0).collect();
    if dir.files_size > 0 {
        let pos = items.partition_point(|(s, _)| *s >= dir.files_size);
        items.insert(pos, (dir.files_size, None));
    }
    if items.is_empty() {
        return;
    }

    let sizes: Vec<f64> = items.iter().map(|(s, _)| *s as f64).collect();
    let rects = squarify(&sizes, inner);
    let top_level_count = items.len().max(1) as f32;

    for (i, ((_, child), child_rect)) in items.iter().zip(rects).enumerate() {
        // Each branch below the view gets its own hue; deeper levels keep it.
        let child_hue = if depth == 0 { i as f32 / top_level_count * 0.85 } else { hue };
        match child {
            Some(c) => layout_dir(tree, *c, child_rect, depth + 1, child_hue, out),
            None if child_rect.width() >= MIN_SIDE && child_rect.height() >= MIN_SIDE => out.push(LaidOutBox {
                rect: child_rect,
                dir: id,
                kind: BoxKind::Files,
                depth: depth + 1,
                hue: child_hue,
                has_header: false,
            }),
            None => {}
        }
    }
}

/// Squarified treemap layout (Bruls, Huizing & van Wijk). `sizes` must be sorted descending.
pub fn squarify(sizes: &[f64], rect: Rect) -> Vec<Rect> {
    let total: f64 = sizes.iter().sum();
    if total <= 0.0 {
        return vec![Rect::NOTHING; sizes.len()];
    }
    let (mut x, mut y) = (rect.min.x as f64, rect.min.y as f64);
    let (mut w, mut h) = (rect.width() as f64, rect.height() as f64);
    let scale = w * h / total;
    let areas: Vec<f64> = sizes.iter().map(|s| s * scale).collect();

    let mut out = Vec::with_capacity(sizes.len());
    let mut start = 0;
    while start < areas.len() {
        let side = w.min(h);
        // Grow the row while it keeps improving the worst aspect ratio.
        let mut end = start + 1;
        let mut row_sum = areas[start];
        let mut worst = worst_ratio(areas[start], areas[start], row_sum, side);
        while end < areas.len() {
            let candidate_sum = row_sum + areas[end];
            let candidate = worst_ratio(areas[start], areas[end], candidate_sum, side);
            if candidate > worst {
                break;
            }
            worst = candidate;
            row_sum = candidate_sum;
            end += 1;
        }

        if w >= h {
            // Row is a vertical strip on the left.
            let strip = if h > 0.0 { row_sum / h } else { 0.0 };
            let mut cy = y;
            for &a in &areas[start..end] {
                let ih = if strip > 0.0 { a / strip } else { 0.0 };
                out.push(rect_f64(x, cy, strip, ih));
                cy += ih;
            }
            x += strip;
            w -= strip;
        } else {
            // Row is a horizontal strip along the top.
            let strip = if w > 0.0 { row_sum / w } else { 0.0 };
            let mut cx = x;
            for &a in &areas[start..end] {
                let iw = if strip > 0.0 { a / strip } else { 0.0 };
                out.push(rect_f64(cx, y, iw, strip));
                cx += iw;
            }
            y += strip;
            h -= strip;
        }
        start = end;
    }
    out
}

fn worst_ratio(largest: f64, smallest: f64, sum: f64, side: f64) -> f64 {
    if sum <= 0.0 || smallest <= 0.0 {
        return f64::INFINITY;
    }
    let side2 = side * side;
    let sum2 = sum * sum;
    (side2 * largest / sum2).max(sum2 / (side2 * smallest))
}

fn rect_f64(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::from_min_max(pos2(x as f32, y as f32), pos2((x + w) as f32, (y + h) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squarify_fills_rect_and_preserves_proportions() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 400.0));
        let sizes = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let rects = squarify(&sizes, rect);
        assert_eq!(rects.len(), sizes.len());
        let total: f64 = sizes.iter().sum();
        for (r, s) in rects.iter().zip(sizes) {
            let expected = s / total * 240_000.0;
            assert!((r.area() as f64 - expected).abs() < 1.0, "{r:?} vs {expected}");
            assert!(rect.expand(0.01).contains_rect(*r));
        }
    }
}
