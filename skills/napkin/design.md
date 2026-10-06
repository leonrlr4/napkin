# Designing a napkin diagram

Read this before drawing anything bigger than a couple of shapes. The mechanics of
`napkin apply` are in SKILL.md; this file is about making the result look deliberate.

## 1. Decide what the picture must say

Write one sentence: "after looking at this, the viewer understands ___". Everything that
doesn't serve that sentence stays out of the drawing (put it in your chat reply instead).

Then pick the one structure that matches the idea, instead of defaulting to a grid of
equal boxes:

| The idea is… | Draw it as |
|---|---|
| a sequence of steps | a single row (or column) of boxes joined by arrows |
| layers that depend downward | horizontal bands stacked top to bottom, arrows only between adjacent bands |
| one thing feeding many | hub on the left, targets fanned out on the right |
| many things feeding one | the mirror image: sources on the left converging on one box |
| a hierarchy | a tree drawn with lines and free-floating text, not nested boxes |
| a loop | boxes around a ring, the last arrow returning to the first |
| two alternatives | two columns side by side with the same row structure |
| a decision | a diamond with labelled exits |

A big diagram may combine two of these (for example a layer stack whose middle layer is a
pipeline), but each region should read as one structure.

## 2. Plan the layout before sending anything

List the nodes, the edges and the groups, then assign coordinates on paper (in your
reasoning) before the first `apply`. Work on a 20 px grid:

- Peers get the same size. Typical node: 180×70 for a one-line label, 180×90 for two lines.
  The single most important node may be larger (240×110) and gets the most empty space
  around it.
- Gaps: 80–120 px between nodes along the flow, 40–60 px between nodes stacked in a
  column, 40 px between a group's border and its contents, 120+ px between groups.
- Align centers: nodes in the same row share `y + height/2`; nodes in the same column
  share `x + width/2`. Misaligned-by-10px boxes are the most visible sign of an
  unprofessional drawing.
- Keep the whole diagram inside `napkin view`'s rectangle, centered, with margin.
- At most about 12 boxes per diagram. If you need more, draw an overview and put the
  detail in a second diagram to the right, or ask the user which part matters.

## 3. Hierarchy comes from type, not from more boxes

- Title: free text, `fontSize` 36, above the diagram, left-aligned with it.
- Group heading: free text, `fontSize` 24, just inside the group's top-left corner (or
  above it).
- Node label: `fontSize` 20, at most three or four words. Put the name in the box, not a
  description.
- Detail and annotations: free text, `fontSize` 16, `strokeColor` `#868e96` (gray),
  placed right next to what it describes. Never go below 16.
- Don't cram sentences into boxes. A box with five lines of 16 px text is the clearest
  sign of an amateur diagram; move the detail out as a gray note or drop it.

## 4. Containers only where they carry meaning

Box only the things arrows connect to. A group ("crates/app", "backend") is one large
rectangle with `backgroundColor` `transparent`, a thin (`strokeWidth` 1) or dashed stroke,
and its heading as free text; the nodes inside are regular boxes. Don't put a box around a
title, a note, or a legend.

## 5. Arrows

- Every relationship you want the viewer to see needs an arrow; position alone doesn't
  say "depends on".
- Keep arrows straight and axis-aligned: connect facing edges (right edge to left edge in
  a row, bottom to top in a column). napkin draws an arrow with more than two points as a
  smooth curve, so you can't make crisp elbows: align the boxes so a straight two-point
  arrow works. Use a curved three-point arrow only for a deliberate loop-back or skip
  connection, never a long diagonal.
- Never let an arrow pass through a box or a label. Move boxes before bending arrows.
- One arrow direction per diagram for the main flow. Secondary relations (tests, optional
  calls, feedback) are dashed (`strokeStyle` `dashed`) and explained in a one-line legend.
- Label an arrow only when the relation isn't obvious: a 16 px gray free text next to the
  arrow's midpoint, not on top of it.

## 6. Color

Color encodes a category, never decoration. Use at most three hues plus gray, from
Excalidraw's palette (stroke / background):

- blue `#1971c2` / `#a5d8ff`, green `#2f9e44` / `#b2f2bb`, violet `#6741d9` / `#d0bfff`,
  yellow `#f08c00` / `#ffec99`, red `#e03131` / `#ffc9c9` (reserve red for errors and
  warnings), gray `#868e96` / `#e9ecef` for supporting or external things.

Pair each box's pale background with the same hue's dark stroke, `fillStyle` `solid`.
Everything in one category gets the same pair. The user may view the canvas in dark mode;
napkin inverts colors there automatically, so always choose for a white background.

## 7. Style

napkin is a sketching tool: the default `roughness` 1 with the hand-drawn font is right
for whiteboarding. When the user asks for something clean, formal or presentation-ready,
use `roughness` 0 on every shape and arrow and `fontFamily` 6 (Nunito) on every label and
text. Don't mix the two styles in one diagram. Keep `opacity` at 100 and `strokeWidth` at
2 (1 for group borders and dividers).

## 8. Review before you report

After the last batch, `napkin render --out /tmp/napkin.png` and Read the image. Check, in
order:

1. Does the structure match the sentence from step 1?
2. A box that grew taller than planned because its label wrapped?
3. Anything overlapping? Any arrow crossing a box or ending in empty space?
4. Uneven gaps or misaligned rows/columns?
5. Text too small to read in the PNG?
6. Lopsided composition: one crowded corner, one empty one?

Fix with `update` ops and render again. Two or three rounds is normal; stop when you'd
show the result to someone without apologizing for it.
