# Pattern strokes

Brush Strokes (I) > Pattern stroke repeats a saved vector motif along a
live curve. The curve keeps its identity, nodes and transform. It is the
editable guide, and only the repeated motifs are drawn.

## Panel and library

1. Select a plain vector object, or several objects with their colours.
   Open Effects > Brush Strokes and choose Pattern stroke.
2. Save selection as motif stores a snapshot in this document's symbol
   library. The original objects stay on the page. Saved motifs are kept
   in `.tdraw` and can be chosen again from the panel's list.
3. Select a curve and Apply to curve, or draw a new stroke with I.
4. To change an existing stroke, select it, choose Pattern stroke and
   Load selected stroke. Change the settings and Apply to curve. This is
   one undo step and replaces the pattern effect without adding a second
   pattern or deleting the guide.
5. A loaded imported motif can also be saved for reuse. Library entries
   and strokes hold independent snapshots: editing a symbol does not
   change existing strokes. Choose the edited entry and apply it again.

Plain paths, rectangles, ellipses, polygons and groups of these are
supported. Convert text and symbol instances to curves first, and break
other live effects apart before saving them as motifs. This first version
repeats rigid motifs; it does not stretch a motif continuously along a
curve. The existing Preset, Brush and Sprayer modes keep their behaviour.

## Parameters and placement

Defaults: size 3 mm, spacing 8 mm, start and end distances 0 mm, side
offset 0 mm, rotation 0 degrees, Follow path direction on, Reverse off.
Size is the longest side of the motif's geometric bounding box, scaled
uniformly. The panel uses the drawing's units for distances; the property
bar shows millimetres. Motif fills and outlines retain their colours.

Each subpath starts a new sequence. Its samples are at
`start + k * spacing`, up to `length - end`, including an open endpoint
when it lands exactly on a sample. Closed subpaths omit a second sample
at the seam. Spacing is centre to centre for native centred motifs.
Imported motifs may retain an offset placement origin.

Lengths and tangents come from the Bezier segments, using arc length
accuracy 0.000001 mm. A copy is placed at the sampled point plus
`offset * left_normal`. It turns by `rotation`, plus the tangent angle
when Follow path direction is on. Reverse measures from the other end,
reverses the tangent and therefore reverses the positive offset side.
The guide's transform is applied to the complete result, so scaling the
guide scales its motif size and spacing together.

Non-finite numbers, nonpositive size or spacing, negative trims and
zero-length paths draw no copies. Motifs have a bounded complexity and
depth, and a stroke draws at most 4096 copies across its subpaths. An
invalid imported source never causes an unbounded generation loop.
Evaluations use the existing live-effect cache, invalidated by a guide,
motif or setting change.

## `.cdr` compatibility

The importer recognises the **observed version 27 type-28 single-motif,
uniform-spacing record**, with a guide/group link in `lnkt`. It decodes
the checked spacing and constant path-relative rotation fields, recovers
the motif's placement origin from the vectors, and checks every placement
against the regenerated result. It keeps the guide and repeated objects
as one live stroke. Unknown flags, other versions, multiple motifs,
nonuniform size or rotation, unmatched guides and mismatched placements
stay as separate vectors, with a warning for an unrecognised type-28
record. They are not assigned guessed settings.

For a recognised import, the original vector placements are kept and
drawn unchanged until the guide, motif or settings change. Editing then
regenerates from the guide and motif. The owned reference drawing's
placements differ from regeneration by at most 0.036 mm; initial import
uses the original placements. This is limited tested compatibility, not
full coverage of every brush or pattern record.

`.tdraw` retains the live parameters and any original placements. SVG,
PDF and legacy RIFF `.cdr` exports use the evaluated vector copies.
Package `.cdr` exports also use the existing TraceDraw source metadata
member, so TraceDraw reopens the effect live. That member does not make
the effect natively editable in other readers. Native linked-pattern
writing and general embedded-motif-library import are not implemented.

## Original examples and checks

[Download the original synthetic examples](../examples/pattern-strokes.zip)
for a bundle of `.tdraw`, `.cdr`, SVG and PNG files.

Run `cargo run -p tracedraw-cli --example pattern_strokes -- /tmp/pattern-strokes`
to generate `.tdraw`, `.cdr`, `.svg` and `.png` examples. They show triangle
markers on a curve at 6 and 10 mm spacing, a 180-degree turn, and a 3 mm
side offset. The source uses original geometry and no external artwork.
Useful cases include decorative borders, repeated diagram markers and
symbols following a curved annotation line.

Focused tests live in `pattern_stroke::tests`, `pattern_record::tests`,
`parse::tests::pattern_stroke_linked_group_recovers_the_guide_and_falls_back_safely`,
`write::tests::pattern_stroke_package_round_trip_keeps_editable_source`, and
`media::tests`. They cover placement, persistence, malformed parameters,
linked import and fallback, apply/edit/undo/redo, and the mode's property bar.
