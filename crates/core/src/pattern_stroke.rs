//! Repeated vector motifs along a live guide, measured in local millimetres.
//! Motifs are snapshots, so deleting or editing their library entry never
//! changes existing drawings. Imported strokes may retain their original placements.

use crate::geometry::{ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg};
use crate::{Affine, BezPath, Point, Shape, ShapeKind, Vec2};
use serde::{Deserialize, Serialize};

/// Maximum copies across all subpaths of one stroke, including malformed input.
pub const MAX_COPIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PatternOptions {
    /// Centre-to-centre distance along the guide.
    pub spacing: f64,
    /// Longest side of the motif's geometric bounding box.
    pub size: f64,
    /// Distance from the beginning and end of each subpath.
    pub start: f64,
    pub end: f64,
    /// Distance to the left of the direction of travel.
    pub offset: f64,
    /// Extra counter-clockwise turn, degrees.
    pub rotation: f64,
    pub follow_tangent: bool,
    pub reverse: bool,
    /// Motif-space placement origin. None centres the motif's geometric box.
    pub anchor: Option<Point>,
}

impl Default for PatternOptions {
    fn default() -> Self {
        Self {
            spacing: 8.0,
            size: 3.0,
            start: 0.0,
            end: 0.0,
            offset: 0.0,
            rotation: 0.0,
            follow_tangent: true,
            reverse: false,
            anchor: None,
        }
    }
}

/// A named reusable motif and its placement origin. Library entries are
/// independent snapshots, so removing one does not change existing strokes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternMotif {
    pub name: String,
    pub shape: Shape,
    #[serde(default)]
    pub anchor: Option<Point>,
}

/// Original imported placements, retained while guide and settings are unchanged.
/// Editing the guide or any parameter regenerates copies from the motif instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternOriginal {
    pub path: BezPath,
    pub options: PatternOptions,
    pub motif: Box<Shape>,
    /// In the guide's local space, in drawing order.
    pub copies: Vec<Shape>,
}

impl PatternOriginal {
    pub fn unchanged(&self, guide: &Shape, motif: &Shape, options: &PatternOptions) -> bool {
        self.copies.len() <= MAX_COPIES
            && self.options == *options
            && self.motif.as_ref() == motif
            && self.path == guide.local_path()
            && self.copies.iter().all(valid_motif)
    }

    pub fn placed(&self, guide: &Shape) -> Vec<Shape> {
        self.copies
            .iter()
            .map(|copy| {
                let mut copy = copy.clone();
                copy.transform = guide.transform * copy.transform;
                copy.opacity *= guide.opacity;
                copy
            })
            .collect()
    }
}

/// Accept only plain vector geometry, with bounded depth and complexity.
/// Text, bitmaps, symbol references and effects need explicit conversion first.
/// This also prevents a motif containing another pattern from expanding itself.
pub fn valid_motif(motif: &Shape) -> bool {
    fn visit(s: &Shape, depth: usize, budget: &mut usize) -> bool {
        if depth > 16 || *budget == 0 || !s.effects.is_empty() || !s.parked_effects.is_empty() {
            return false;
        }
        *budget -= 1;
        if !s.transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return false;
        }
        match &s.kind {
            ShapeKind::Path { path, .. } => {
                let n = path.elements().len();
                if n > *budget || !path.is_finite() {
                    return false;
                }
                *budget -= n;
                true
            }
            ShapeKind::Polygon { points, .. } if *points > 1024 => false,
            ShapeKind::Rect { .. } | ShapeKind::Ellipse { .. } | ShapeKind::Polygon { .. } => {
                s.local_path().is_finite()
            }
            ShapeKind::Group { children } => {
                !children.is_empty() && children.iter().all(|c| visit(c, depth + 1, budget))
            }
            _ => false,
        }
    }
    visit(motif, 0, &mut 4096)
}

/// Generate local placements, then carry them through the guide's transform.
/// The original path, its nodes, motif colours and child transforms are kept.
/// Invalid parameters draw no copies; a zero length guide also draws none.
pub fn copies(guide: &Shape, motif: &Shape, o: &PatternOptions) -> Vec<Shape> {
    if !valid_motif(motif)
        || ![o.spacing, o.size, o.start, o.end, o.offset, o.rotation]
            .iter()
            .all(|v| v.is_finite())
        || o.spacing <= 0.0
        || o.size <= 0.0
        || o.start < 0.0
        || o.end < 0.0
    {
        return Vec::new();
    }
    let b = motif.bounds();
    let extent = b.width().max(b.height());
    if !extent.is_finite() || extent <= 1e-9 || !b.center().is_finite() {
        return Vec::new();
    }
    let scale = o.size / extent;
    let anchor = o.anchor.unwrap_or_else(|| b.center());
    if !anchor.is_finite() {
        return Vec::new();
    }
    let normalize = Affine::scale(scale) * Affine::translate(-anchor.to_vec2());
    let mut out = Vec::new();
    for (path, closed) in crate::effects::subpaths(&guide.local_path()) {
        if !path.is_finite() {
            continue;
        }
        let segments: Vec<_> = path
            .segments()
            .map(|s| (s, s.arclen(1e-6)))
            .filter(|(_, length)| length.is_finite() && *length > 1e-9)
            .collect();
        let length: f64 = segments.iter().map(|(_, length)| length).sum();
        let stop = length - o.end;
        if !length.is_finite() || length <= 0.0 || stop < o.start {
            continue;
        }
        // Index multiplication cannot stall like repeated floating-point addition.
        let count = (((stop - o.start) / o.spacing).floor() as usize)
            .saturating_add(1)
            .min(MAX_COPIES - out.len());
        for i in 0..count {
            let distance = o.start + i as f64 * o.spacing;
            // A closed guide has one copy at the seam, never two.
            if closed && distance >= length - 1e-9 {
                break;
            }
            let target = if o.reverse {
                length - distance
            } else {
                distance
            };
            let mut remaining = target;
            let mut sample = None;
            for (segment, segment_length) in &segments {
                if remaining <= *segment_length + 1e-9 {
                    let t = segment.inv_arclen(remaining.clamp(0.0, *segment_length), 1e-6);
                    let point = segment.eval(t);
                    let direction = match segment {
                        PathSeg::Line(s) => s.p1 - s.p0,
                        PathSeg::Quad(s) => s.deriv().eval(t).to_vec2(),
                        PathSeg::Cubic(s) => s.deriv().eval(t).to_vec2(),
                    };
                    let magnitude = direction.hypot();
                    if magnitude > 1e-12 {
                        sample = Some((
                            point,
                            direction / magnitude * if o.reverse { -1.0 } else { 1.0 },
                        ));
                    }
                    break;
                }
                remaining -= segment_length;
            }
            let Some((point, tangent)) = sample else {
                continue;
            };
            let point = point + Vec2::new(-tangent.y, tangent.x) * o.offset;
            let angle = o.rotation.to_radians()
                + if o.follow_tangent {
                    tangent.atan2()
                } else {
                    0.0
                };
            let mut copy = motif.clone();
            copy.transform = guide.transform
                * Affine::translate(point.to_vec2())
                * Affine::rotate(angle)
                * normalize
                * copy.transform;
            copy.opacity *= guide.opacity;
            out.push(copy);
        }
        if out.len() == MAX_COPIES {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{live::Effect, BezPath, Color, Document, Fill, Point, Rect, ShapeId, Size};

    fn motif() -> Shape {
        let mut s = Shape::new(
            ShapeId(2),
            ShapeKind::Path {
                path: crate::geometry::rect_path(Rect::new(-2.0, -1.0, 2.0, 1.0), 0.0),
                closed: true,
            },
        );
        s.fill = Fill::Solid(Color::rgb8(210, 30, 70));
        s.stroke = None;
        s
    }

    fn guide() -> Shape {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((40.0, 0.0));
        Shape::new(
            ShapeId(1),
            ShapeKind::Path {
                path,
                closed: false,
            },
        )
    }

    #[test]
    fn pattern_motif_library_is_undoable_and_keeps_an_explicit_empty_library() {
        let mut doc = Document::new("Motifs", Size::new(100.0, 100.0));
        doc.symbols.push(crate::Symbol {
            name: "Legacy symbol".into(),
            shape: motif(),
        });
        let mut engine = crate::Engine::new(doc);
        let entry = PatternMotif {
            name: "Marker".into(),
            shape: motif(),
            anchor: Some(Point::ZERO),
        };
        engine
            .run(&crate::Command::SetPatternMotifs {
                motifs: vec![entry.clone()],
            })
            .unwrap();
        let back = Document::from_json(&engine.document().to_json().unwrap()).unwrap();
        assert_eq!(back.pattern_motifs, Some(vec![entry]));
        engine
            .run(&crate::Command::SetPatternMotifs { motifs: Vec::new() })
            .unwrap();
        let back = Document::from_json(&engine.document().to_json().unwrap()).unwrap();
        assert_eq!(back.pattern_motifs, Some(Vec::new()));
        assert_eq!(back.symbols.len(), 1);
        engine.undo().unwrap();
        assert_eq!(engine.document().pattern_motifs.as_ref().unwrap().len(), 1);
        engine.undo().unwrap();
        assert!(engine.document().pattern_motifs.is_none());
        engine.redo().unwrap();
        assert_eq!(engine.document().pattern_motifs.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn pattern_stroke_spacing_and_trims_are_in_millimetres() {
        let o = PatternOptions {
            size: 4.0,
            ..Default::default()
        };
        let g = guide();
        let m = motif();
        let result = copies(&g, &m, &o);
        assert_eq!(result.len(), 6);
        for (i, c) in result.iter().enumerate() {
            assert!((c.bounds().center().x - i as f64 * 8.0).abs() < 1e-9);
            assert_eq!(c.fill, m.fill);
        }
        let trimmed = copies(
            &g,
            &m,
            &PatternOptions {
                start: 4.0,
                end: 4.0,
                offset: 2.0,
                ..o
            },
        );
        assert_eq!(trimmed.len(), 5);
        assert_eq!(trimmed[0].bounds().center(), Point::new(4.0, 2.0));
    }

    #[test]
    fn pattern_stroke_turns_reverses_and_carries_the_guide_transform() {
        let mut g = guide();
        g.transform = Affine::translate((20.0, 30.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
        let o = PatternOptions {
            size: 4.0,
            offset: 2.0,
            reverse: true,
            ..Default::default()
        };
        let result = copies(&g, &motif(), &o);
        assert!((result[0].bounds().center() - Point::new(22.0, 70.0)).hypot() < 1e-9);
        assert!((result[0].bounds().height() - 4.0).abs() < 1e-9);
        let fixed = copies(
            &guide(),
            &motif(),
            &PatternOptions {
                size: 4.0,
                rotation: 90.0,
                follow_tangent: false,
                ..Default::default()
            },
        );
        assert!((fixed[0].bounds().height() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn pattern_stroke_closed_seam_and_disjoint_subpaths() {
        let mut g = guide();
        let mut path = crate::geometry::rect_path(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0);
        path.move_to((100.0, 0.0));
        path.line_to((110.0, 0.0));
        g.kind = ShapeKind::Path {
            path,
            closed: false,
        };
        let result = copies(
            &g,
            &motif(),
            &PatternOptions {
                spacing: 10.0,
                ..Default::default()
            },
        );
        assert_eq!(result.len(), 6); // four on the ring, two on the separate line
        assert_eq!(result[4].bounds().center(), Point::new(100.0, 0.0));
    }

    #[test]
    fn pattern_stroke_rejects_invalid_input_and_bounds_the_work() {
        let g = guide();
        let m = motif();
        for spacing in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(copies(
                &g,
                &m,
                &PatternOptions {
                    spacing,
                    ..Default::default()
                }
            )
            .is_empty());
        }
        assert_eq!(
            copies(
                &g,
                &m,
                &PatternOptions {
                    spacing: f64::MIN_POSITIVE,
                    ..Default::default()
                }
            )
            .len(),
            MAX_COPIES
        );
        let mut recursive = m.clone();
        recursive.effects.push(Effect::PatternStroke {
            motif: Box::new(m.clone()),
            options: Default::default(),
            original: None,
        });
        assert!(copies(&g, &recursive, &Default::default()).is_empty());
        let mut empty = g.clone();
        empty.kind = ShapeKind::Path {
            path: BezPath::new(),
            closed: false,
        };
        assert!(copies(&empty, &m, &Default::default()).is_empty());
        let bad = r#"{"spacing":"eight"}"#;
        assert!(serde_json::from_str::<PatternOptions>(bad).is_err());
        assert_eq!(
            serde_json::from_str::<PatternOptions>("{}").unwrap(),
            PatternOptions::default()
        );
    }

    #[test]
    fn pattern_stroke_keeps_original_placements_until_a_real_edit() {
        let mut g = guide();
        let m = motif();
        let options = PatternOptions::default();
        let mut original = copies(&g, &m, &options);
        original[0].transform = Affine::translate((0.01, 0.0)) * original[0].transform;
        g.effects.push(Effect::PatternStroke {
            motif: Box::new(m.clone()),
            options: options.clone(),
            original: Some(Box::new(PatternOriginal {
                path: g.local_path(),
                options,
                motif: Box::new(m),
                copies: original.clone(),
            })),
        });
        assert_eq!(crate::live::evaluate(&g).above, original);
        let mut doc = Document::new("Imported pattern", Size::new(100.0, 100.0));
        doc.pages[0].layers[0].shapes.push(g.clone());
        let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(
            crate::live::evaluate(&back.pages[0].layers[0].shapes[0]).above,
            original
        );
        if let Effect::PatternStroke { options, .. } = &mut g.effects[0] {
            options.spacing = 10.0;
        }
        let edited = crate::live::evaluate(&g);
        assert_eq!(edited.above.len(), 5);
        assert_eq!(edited.above[0].bounds().center(), Point::ZERO);
    }

    /// Run only when checking pattern evaluation cost, never as a timing gate.
    #[test]
    #[ignore]
    fn pattern_stroke_frame_times() {
        let mut g = guide();
        if let ShapeKind::Path { path, .. } = &mut g.kind {
            path.line_to((960.0, 0.0));
        }
        g.effects.push(Effect::PatternStroke {
            motif: Box::new(motif()),
            options: PatternOptions::default(),
            original: None,
        });
        let start = std::time::Instant::now();
        let first = crate::live::evaluate(&g);
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            assert_eq!(crate::live::evaluate(&g).above.len(), first.above.len());
        }
        eprintln!(
            "pattern: {} copies; cold {:?}; cached {:?} per evaluation",
            first.above.len(),
            cold,
            start.elapsed() / 100
        );
    }

    #[test]
    fn pattern_stroke_remains_live_and_round_trips() {
        let mut g = guide();
        let original = g.kind.clone();
        g.effects.push(Effect::PatternStroke {
            motif: Box::new(motif()),
            options: Default::default(),
            original: None,
        });
        let first = crate::live::evaluate(&g);
        assert_eq!(first.above.len(), 6);
        assert_eq!(first.main.kind, original);
        assert_eq!(first.main.fill, Fill::None);
        assert!(first.main.stroke.is_none());
        let mut doc = Document::new("Pattern stroke", Size::new(100.0, 100.0));
        doc.pages[0].layers[0].shapes.push(g.clone());
        let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(back.pages[0].layers[0].shapes[0], g);
        if let ShapeKind::Path { path, .. } = &mut g.kind {
            path.line_to((80.0, 0.0));
        }
        assert_eq!(crate::live::evaluate(&g).above.len(), 11);
        assert_eq!(crate::live::evaluate(&g).above.len(), 11); // cached evaluation
    }
}
