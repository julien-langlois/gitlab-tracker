//! Line icons painted with egui shapes, on a 24×24 grid scaled to the target rect.

use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    MergeRequests,
    #[cfg(feature = "stats")]
    Stats,
    Settings,
    PanelLeft,
    Columns,
    Help,
    PanelRight,
    Refresh,
    Copy,
    Star,
    StarOutline,
    Sun,
    Moon,
    Minimize,
    Maximize,
    Close,
    Home,
    Plus,
    Search,
    Comment,
    Cockpit,
    Ticket,
    ExternalLink,
    Clipboard,
    ArrowDown,
    ArrowUp,
    Clock,
}

/// Paints `icon` centered in `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let origin = rect.center() - egui::vec2(side, side) / 2.0;
    let scale = side / 24.0;
    let p = |x: f32, y: f32| -> Pos2 { origin + egui::vec2(x, y) * scale };
    let stroke = Stroke::new((1.7 * scale).max(1.0), color);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], stroke);
    };
    let circle = |c: Pos2, r: f32| painter.circle_stroke(c, r * scale, stroke);

    match icon {
        Icon::MergeRequests => {
            circle(p(6.0, 6.0), 2.5);
            circle(p(6.0, 18.0), 2.5);
            circle(p(18.0, 18.0), 2.5);
            line(p(6.0, 8.5), p(6.0, 15.5));
            painter.add(Shape::line(
                vec![p(6.0, 8.5), p(9.0, 12.0), p(15.0, 12.5), p(18.0, 15.5)],
                stroke,
            ));
        }
        #[cfg(feature = "stats")]
        Icon::Stats => {
            line(p(5.0, 20.0), p(5.0, 11.0));
            line(p(11.0, 20.0), p(11.0, 5.0));
            line(p(17.0, 20.0), p(17.0, 13.0));
            line(p(3.0, 20.0), p(21.0, 20.0));
        }
        Icon::Settings => {
            circle(p(12.0, 12.0), 3.0);
            for (a, b) in [
                ((12.0, 2.5), (12.0, 5.5)),
                ((12.0, 18.5), (12.0, 21.5)),
                ((2.5, 12.0), (5.5, 12.0)),
                ((18.5, 12.0), (21.5, 12.0)),
                ((5.3, 5.3), (7.4, 7.4)),
                ((16.6, 16.6), (18.7, 18.7)),
                ((5.3, 18.7), (7.4, 16.6)),
                ((16.6, 7.4), (18.7, 5.3)),
            ] {
                line(p(a.0, a.1), p(b.0, b.1));
            }
        }
        Icon::PanelLeft | Icon::PanelRight => {
            let frame = Rect::from_min_max(p(3.0, 4.0), p(21.0, 20.0));
            painter.rect_stroke(frame, 2.0 * scale, stroke, egui::StrokeKind::Middle);
            let x = if icon == Icon::PanelLeft { 9.0 } else { 15.0 };
            line(p(x, 4.0), p(x, 20.0));
        }
        Icon::Columns => {
            let frame = Rect::from_min_max(p(3.0, 4.0), p(21.0, 20.0));
            painter.rect_stroke(frame, 2.0 * scale, stroke, egui::StrokeKind::Middle);
            line(p(9.0, 4.0), p(9.0, 20.0));
            line(p(15.0, 4.0), p(15.0, 20.0));
        }
        Icon::Help => {
            circle(p(12.0, 12.0), 9.0);
            painter.add(Shape::line(
                vec![
                    p(9.5, 9.5),
                    p(10.5, 7.5),
                    p(13.5, 7.5),
                    p(14.5, 9.5),
                    p(12.0, 12.0),
                    p(12.0, 14.0),
                ],
                stroke,
            ));
            painter.circle_filled(p(12.0, 17.0), 1.1 * scale, color);
        }
        Icon::Refresh => {
            let center = p(12.0, 12.0);
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let t = -0.3 + (i as f32 / 20.0) * 5.2;
                    center + egui::vec2(t.cos(), t.sin()) * 7.5 * scale
                })
                .collect();
            painter.add(Shape::line(points, stroke));
            painter.add(Shape::line(
                vec![p(19.5, 4.5), p(19.5, 10.5), p(13.5, 10.5)],
                stroke,
            ));
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(8.0, 8.0), p(19.0, 20.0)),
                2.0 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.add(Shape::line(
                vec![p(5.0, 16.0), p(5.0, 4.0), p(15.0, 4.0)],
                stroke,
            ));
        }
        Icon::Star => {
            // The star is not convex: fill it as a fan of triangles from its center.
            let center = p(12.0, 12.5);
            let points = star(center, 9.0 * scale);
            for i in 0..points.len() {
                let next = points[(i + 1) % points.len()];
                painter.add(Shape::convex_polygon(
                    vec![center, points[i], next],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::StarOutline => {
            painter.add(Shape::closed_line(star(p(12.0, 12.5), 9.0 * scale), stroke));
        }
        Icon::Sun => {
            circle(p(12.0, 12.0), 4.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (s, c) = a.sin_cos();
                line(
                    p(12.0 + c * 7.0, 12.0 + s * 7.0),
                    p(12.0 + c * 9.5, 12.0 + s * 9.5),
                );
            }
        }
        Icon::Moon => {
            let center = p(12.0, 12.0);
            let points: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = 0.9 + (i as f32 / 24.0) * 4.5;
                    center + egui::vec2(t.cos(), t.sin()) * 8.0 * scale
                })
                .collect();
            painter.add(Shape::line(points, stroke));
            painter.add(Shape::line(
                vec![p(17.0, 5.0), p(13.0, 9.0), p(14.0, 14.5), p(19.0, 17.0)],
                stroke,
            ));
        }
        Icon::Minimize => {
            line(p(6.0, 12.0), p(18.0, 12.0));
        }
        Icon::Maximize => {
            painter.rect_stroke(
                Rect::from_min_max(p(6.0, 6.0), p(18.0, 18.0)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Icon::Close => {
            line(p(6.0, 6.0), p(18.0, 18.0));
            line(p(18.0, 6.0), p(6.0, 18.0));
        }
        Icon::Home => {
            painter.add(Shape::closed_line(
                vec![
                    p(3.0, 10.5),
                    p(12.0, 3.0),
                    p(21.0, 10.5),
                    p(21.0, 21.0),
                    p(15.0, 21.0),
                    p(15.0, 15.0),
                    p(9.0, 15.0),
                    p(9.0, 21.0),
                    p(3.0, 21.0),
                ],
                stroke,
            ));
        }
        Icon::Plus => {
            line(p(12.0, 5.0), p(12.0, 19.0));
            line(p(5.0, 12.0), p(19.0, 12.0));
        }
        Icon::Search => {
            circle(p(11.0, 11.0), 6.0);
            line(p(15.5, 15.5), p(20.0, 20.0));
        }
        Icon::Comment => {
            painter.add(Shape::closed_line(
                vec![
                    p(4.0, 5.0),
                    p(20.0, 5.0),
                    p(20.0, 16.0),
                    p(9.0, 16.0),
                    p(4.0, 20.0),
                ],
                stroke,
            ));
        }
        Icon::Cockpit => {
            let center = p(12.0, 18.0);
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let t = std::f32::consts::PI * (1.0 + i as f32 / 20.0);
                    center + egui::vec2(t.cos(), t.sin()) * 8.0 * scale
                })
                .collect();
            painter.add(Shape::line(points, stroke));
            line(center, p(16.0, 12.0));
        }
        Icon::Ticket => {
            // A ticket stub: notches on both sides, a dashed tear line.
            painter.add(Shape::closed_line(
                vec![
                    p(3.0, 7.0),
                    p(21.0, 7.0),
                    p(21.0, 10.0),
                    p(19.5, 12.0),
                    p(21.0, 14.0),
                    p(21.0, 17.0),
                    p(3.0, 17.0),
                    p(3.0, 14.0),
                    p(4.5, 12.0),
                    p(3.0, 10.0),
                ],
                stroke,
            ));
            for y in [8.5, 11.5, 14.5] {
                line(p(14.0, y), p(14.0, y + 1.5));
            }
        }
        Icon::ExternalLink => {
            painter.add(Shape::line(
                vec![
                    p(18.0, 14.0),
                    p(18.0, 20.0),
                    p(4.0, 20.0),
                    p(4.0, 6.0),
                    p(10.0, 6.0),
                ],
                stroke,
            ));
            painter.add(Shape::line(
                vec![p(14.0, 4.0), p(20.0, 4.0), p(20.0, 10.0)],
                stroke,
            ));
            line(p(20.0, 4.0), p(11.0, 13.0));
        }
        Icon::Clipboard => {
            painter.rect_stroke(
                Rect::from_min_max(p(6.0, 4.0), p(18.0, 21.0)),
                2.0 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(9.0, 3.0), p(15.0, 6.0)),
                1.0 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Icon::Clock => {
            circle(p(12.0, 12.0), 8.5);
            painter.add(Shape::line(
                vec![p(12.0, 7.5), p(12.0, 12.0), p(15.0, 14.0)],
                stroke,
            ));
        }
        Icon::ArrowDown | Icon::ArrowUp => {
            let (from, to, wing) = if icon == Icon::ArrowDown {
                (5.0, 19.0, 13.0)
            } else {
                (19.0, 5.0, 11.0)
            };
            line(p(12.0, from), p(12.0, to));
            painter.add(Shape::line(
                vec![p(6.0, wing), p(12.0, to), p(18.0, wing)],
                stroke,
            ));
        }
    }
}

/// The ten corners of a five-pointed star, outer and inner alternating.
fn star(center: Pos2, radius: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { radius } else { radius * 0.45 };
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            pos2(center.x + a.cos() * r, center.y + a.sin() * r)
        })
        .collect()
}
