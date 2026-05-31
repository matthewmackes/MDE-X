//! VIRT-17.a — a tiny CPU/RAM sparkline for the VM detail panel.
//!
//! A 60-point ring buffer feeds an iced `canvas` line chart (no axes —
//! just the trend shape, auto-scaled to the data's max). The pure parts
//! (`push_sample`, `spark_points`) are unit-tested; the canvas `Program`
//! renders them.
//!
//! Cite: visual-identity.md §1; ref: Apple System Settings.

use std::collections::VecDeque;

use iced::widget::canvas::{self, Frame, Geometry, Path, Program, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};

/// Ring-buffer capacity (≈2 min at the sample cadence).
pub const SPARK_CAP: usize = 60;

/// Push a sample, dropping the oldest when the buffer is full.
pub fn push_sample(buf: &mut VecDeque<f32>, v: f32) {
    if buf.len() >= SPARK_CAP {
        buf.pop_front();
    }
    buf.push_back(v);
}

/// Map values to `(x, y)` points inside a `w × h` box: x spreads evenly,
/// y is inverted (0 at the bottom) and auto-scaled to the data's max
/// (floored at 1.0 to avoid divide-by-zero on an all-zero series).
/// Pure. Returns empty for fewer than 2 points (nothing to draw).
pub fn spark_points(values: &[f32], w: f32, h: f32) -> Vec<(f32, f32)> {
    if values.len() < 2 {
        return vec![];
    }
    let max = values.iter().copied().fold(0.0_f32, f32::max).max(1.0);
    let n = values.len();
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = w * (i as f32) / ((n - 1) as f32);
            let y = h - (v / max).clamp(0.0, 1.0) * h;
            (x, y)
        })
        .collect()
}

/// The canvas program: a single stroked polyline over `values`.
struct Spark {
    values: Vec<f32>,
    color: Color,
}

impl<Message> Program<Message> for Spark {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let pts = spark_points(&self.values, frame.width(), frame.height());
        if pts.len() >= 2 {
            let path = Path::new(|b| {
                for (i, &(x, y)) in pts.iter().enumerate() {
                    let p = Point::new(x, y);
                    if i == 0 {
                        b.move_to(p);
                    } else {
                        b.line_to(p);
                    }
                }
            });
            frame.stroke(
                &path,
                Stroke::default().with_color(self.color).with_width(1.5),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// Build a fixed-height sparkline `Element` from a buffer's values.
pub fn sparkline<'a, Message: 'a>(
    values: Vec<f32>,
    color: Color,
    height: f32,
) -> Element<'a, Message> {
    canvas::Canvas::new(Spark { values, color })
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_sample_wraps_at_capacity() {
        let mut buf = VecDeque::new();
        for i in 0..(SPARK_CAP + 5) {
            push_sample(&mut buf, i as f32);
        }
        assert_eq!(buf.len(), SPARK_CAP);
        // The first 5 samples (0..5) were dropped; oldest is now 5.0.
        assert_eq!(*buf.front().unwrap(), 5.0);
        assert_eq!(*buf.back().unwrap(), (SPARK_CAP + 4) as f32);
    }

    #[test]
    fn spark_points_maps_to_box() {
        // Two points: first at x=0, last at x=w. y inverted + auto-scaled.
        let pts = spark_points(&[0.0, 100.0], 200.0, 40.0);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].0, 0.0); // first x at left edge
        assert_eq!(pts[1].0, 200.0); // last x at right edge
        assert_eq!(pts[0].1, 40.0); // value 0 → bottom (y = h)
        assert_eq!(pts[1].1, 0.0); // value max → top (y = 0)
    }

    #[test]
    fn spark_points_empty_below_two() {
        assert!(spark_points(&[], 100.0, 40.0).is_empty());
        assert!(spark_points(&[5.0], 100.0, 40.0).is_empty());
    }
}
