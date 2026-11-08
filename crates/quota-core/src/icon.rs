//! Tray icon: the brand's ring gauge. The arc length is the remaining share of the tightest
//! window, its colour the alert level; the battery in the middle mirrors the same fill.

use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::model::Level;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum IconState {
    /// Remaining percent (0-100) of the tightest window and its level.
    Gauge { remaining: f64, level: Level, issue: bool },
    /// No provider data yet.
    Idle,
    /// Every provider failed.
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconStyle {
    /// Full colour, readable on light and dark trays.
    Color,
    /// Black-only alpha mask for macOS template images (the menu bar tints it).
    Template,
}

const GREEN: (u8, u8, u8) = (0x2F, 0xB3, 0x6B);
const AMBER: (u8, u8, u8) = (0xF0, 0x9A, 0x1A);
const RED: (u8, u8, u8) = (0xE5, 0x48, 0x4D);
const GRAY: (u8, u8, u8) = (0x8A, 0x8F, 0x98);

fn rgba(c: (u8, u8, u8), a: f32) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, (a * 255.0) as u8)
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

/// Renders a square RGBA icon (`size` px, straight alpha) ready for the tray.
pub fn render(state: IconState, style: IconStyle, size: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(size, size).expect("icon size");
    let s = size as f32;
    let stroke_w = s * 0.115;
    let r = s / 2.0 - stroke_w / 2.0 - s * 0.03;
    let (cx, cy) = (s / 2.0, s / 2.0);

    let mono = style == IconStyle::Template;
    let ink = |c: (u8, u8, u8), a: f32| if mono { Color::from_rgba8(0, 0, 0, (a * 255.0) as u8) } else { rgba(c, a) };

    let (fraction, color, badge) = match state {
        IconState::Gauge { remaining, level, issue } => {
            let c = match level {
                Level::Normal => GREEN,
                Level::Warning => AMBER,
                Level::Critical => RED,
            };
            ((remaining / 100.0).clamp(0.0, 1.0) as f32, c, issue)
        }
        IconState::Idle => (0.0, GRAY, false),
        IconState::Error => (0.0, GRAY, true),
    };

    // Track.
    let mut stroke = Stroke { width: stroke_w, line_cap: LineCap::Round, ..Stroke::default() };
    if let Some(path) = arc(cx, cy, r, 0.0, 1.0) {
        pm.stroke_path(&path, &paint(ink(GRAY, if mono { 0.28 } else { 0.38 })), &stroke, Transform::identity(), None);
    }
    // Value arc, clockwise from 12 o'clock, with the brand's end dot.
    if fraction > 0.004 {
        if let Some(path) = arc(cx, cy, r, 0.0, fraction) {
            pm.stroke_path(&path, &paint(ink(color, 1.0)), &stroke, Transform::identity(), None);
        }
        let a = -std::f32::consts::FRAC_PI_2 + fraction * std::f32::consts::TAU;
        if let Some(dot) = PathBuilder::from_circle(cx + r * a.cos(), cy + r * a.sin(), stroke_w * 0.92) {
            pm.fill_path(&dot, &paint(ink(color, 1.0)), FillRule::Winding, Transform::identity(), None);
        }
    }

    // Battery body + cap + level fill.
    let bw = s * 0.40;
    let bh = s * 0.23;
    let bx = cx - bw / 2.0 - s * 0.02;
    let by = cy - bh / 2.0;
    stroke.width = (s * 0.055).max(1.0);
    stroke.line_cap = LineCap::Butt;
    let body_ink = if matches!(state, IconState::Gauge { .. }) { ink(color, 1.0) } else { ink(GRAY, 0.9) };
    if let Some(body) = rounded_rect(bx, by, bw, bh, s * 0.05) {
        pm.stroke_path(&body, &paint(body_ink), &stroke, Transform::identity(), None);
    }
    if let Some(cap) = Rect::from_xywh(bx + bw + stroke.width * 0.4, cy - bh * 0.22, s * 0.04, bh * 0.44) {
        pm.fill_rect(cap, &paint(body_ink), Transform::identity(), None);
    }
    let inset = stroke.width * 1.15;
    let fill_w = (bw - inset * 2.0) * fraction.max(if fraction > 0.0 { 0.12 } else { 0.0 });
    if let Some(level) = Rect::from_xywh(bx + inset, by + inset, fill_w, bh - inset * 2.0) {
        pm.fill_rect(level, &paint(body_ink), Transform::identity(), None);
    }

    // Issue badge: a solid dot at the top-right, knocked out of the ring.
    if badge {
        let (bx, by, br) = (s * 0.80, s * 0.20, s * 0.16);
        if let Some(knock) = PathBuilder::from_circle(bx, by, br + s * 0.05) {
            let clear = Paint { blend_mode: tiny_skia::BlendMode::Clear, ..Paint::default() };
            pm.fill_path(&knock, &clear, FillRule::Winding, Transform::identity(), None);
        }
        if let Some(dot) = PathBuilder::from_circle(bx, by, br) {
            pm.fill_path(&dot, &paint(ink(RED, 1.0)), FillRule::Winding, Transform::identity(), None);
        }
    }

    unpremultiply(pm)
}

fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let steps = ((to - from) * 96.0).ceil().max(2.0) as usize;
    for i in 0..=steps {
        let t = from + (to - from) * i as f32 / steps as f32;
        let a = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::TAU;
        let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
        if i == 0 {
            pb.move_to(x, y)
        } else {
            pb.line_to(x, y)
        }
    }
    pb.finish()
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn unpremultiply(pm: Pixmap) -> Vec<u8> {
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

/// PNG bytes, used for previews and tests.
pub fn render_png(state: IconState, style: IconStyle, size: u32) -> Vec<u8> {
    let rgba = render(state, style, size);
    let mut pm = Pixmap::new(size, size).unwrap();
    for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    pm.encode_png().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_states() {
        let states = [
            ("normal", IconState::Gauge { remaining: 72.0, level: Level::Normal, issue: false }),
            ("warning", IconState::Gauge { remaining: 16.0, level: Level::Warning, issue: false }),
            ("critical", IconState::Gauge { remaining: 6.0, level: Level::Critical, issue: false }),
            ("stale", IconState::Gauge { remaining: 54.0, level: Level::Normal, issue: true }),
            ("idle", IconState::Idle),
            ("error", IconState::Error),
        ];
        let out = std::env::var("ICON_PREVIEW_DIR").ok();
        for (name, st) in states {
            for style in [IconStyle::Color, IconStyle::Template] {
                let px = render(st, style, 64);
                assert_eq!(px.len(), 64 * 64 * 4);
                assert!(px.iter().skip(3).step_by(4).any(|a| *a > 0), "{name} is blank");
                if let Some(dir) = &out {
                    let tag = if style == IconStyle::Color { "color" } else { "template" };
                    std::fs::write(format!("{dir}/{name}-{tag}.png"), render_png(st, style, 64)).unwrap();
                }
            }
        }
    }
}
