---
name: napkin
description: Read, draw on and look at the napkin whiteboard the user has open. Use when the user asks to draw, sketch, diagram or visualize something in napkin, to explain or implement from a drawing on the napkin canvas, or mentions "napkin", "the canvas" or "the whiteboard".
---

napkin is a whiteboard the user keeps open on screen. The `napkin` command talks to the
running instance over its control socket and edits the file the user is looking at right
now: every batch you apply shows up on their canvas immediately, and one `Ctrl+Z` on their
end undoes one whole batch.

## Before drawing

Run `napkin status` first: which file is open (or `(none)` for an unsaved canvas) and
whether it is read-only. Then `napkin view` to see the rectangle the user can actually see
on screen, in scene coordinates, and put new content inside it (or tell the user you're
drawing off-screen and why). Run `napkin scene` to see what is already there so new shapes
don't land on top of existing ones. Don't move or restyle the user's own elements unless
they ask you to.

## Reading

`napkin scene` and `napkin selection` print one header line, `file <path>` (or
`file (none)`), then one line per element:

```
<id> <type> <x> <y> <w> <h>
```

followed by whichever of these apply, in this order: `label="..."` (the bound text of a
container, folded into the container's line rather than shown on its own), `text="..."`
(a standalone text element's own content), `stroke=<color>` (omitted when it's the default
`#1e1e1e`), `bg=<color>` (omitted when `transparent`), `start=<id>` / `end=<id>` (an
arrow's bindings), `points=<JSON>` (lines and arrows only), `groups=<id,id>`,
`angle=<radians>` (omitted when 0) and `locked`. Numbers are rounded to 2 decimals.
`stroke=`/`bg=` only ever show the color; fill style, stroke style, stroke width and other
styling only show up in `--full` or `render`. Example, after drawing a small flow:

```
file /home/user/Documents/napkin/demo.excalidraw
V1StGXR8_Z5jdHi6B-my1 rectangle 40 40 160 70 label="Request" stroke=#1971c2 bg=#a5d8ff
V1StGXR8_Z5jdHi6B-my2 rectangle 300 40 160 70 label="Process" stroke=#1971c2 bg=#a5d8ff
V1StGXR8_Z5jdHi6B-my3 arrow 206.5 75 88 0 start=V1StGXR8_Z5jdHi6B-my1 end=V1StGXR8_Z5jdHi6B-my2 points=[[0,0],[87,0]]
```

`napkin selection` shows only what the user has selected (plus any bound labels), with the
same format; an empty selection prints just the `file` line. Add `--full` to either to get
one compact-JSON element per line instead, with every field: use it when you need a field
the summary line doesn't show, such as `fontFamily` or `strokeStyle`.

To actually look at the drawing (the user's own hand-drawn sketch, or to sanity-check
your own layout), render it to PNG and then `Read` that file:
`napkin render --out /tmp/napkin-selection.png --selection` when something is selected,
otherwise `napkin render --out /tmp/napkin.png` for the whole scene.

## Drawing

Send a batch on stdin:

```
napkin apply <<'EOF'
{"ops": [ ... ]}
EOF
```

Each op is `{"op": "add", ...}`, `{"op": "update", "id": "...", "set": {...}}` or
`{"op": "delete", "ids": ["..."]}`. Within one batch, every `add` runs before any
`update`/`delete`, regardless of the order you wrote them in, so an `update`/`delete` can
target an `add` that appears later in the same batch. Give every `add` you'll need to
reference (as an arrow's `start`/`end`, or by a later `update`/`delete` in the *same*
batch) an `id` of your choosing; it's a batch-local alias, not the element's real id. The
response's `created` map gives you the real, permanent id for each alias
(`{"created": {"your-alias": "<real id>"}}`) — use those real ids in later, separate
batches, since aliases don't carry over.

`add` fields, by `type`:

- `rectangle` / `diamond` / `ellipse`: `x`, `y`, `width` (> 0), `height` (> 0), optionally
  `label: {"text": "...", "fontSize": ..., "fontFamily": ..., "textAlign": ...,
  "verticalAlign": ...}`. Both `width` and `height` are required; napkin never grows a
  shape to fit its label, so size the box yourself (see Layout below) and check for the
  `warnings` a too-small box produces.
- `text`: `x`, `y`, `text` (required), optionally `fontSize`, `fontFamily`, `textAlign`
  (`left`/`center`/`right`, default `left`). `y` is always the top of the text; there is no
  `verticalAlign` for a standalone text element. `x` is the anchor `textAlign` describes,
  not always the left edge: with `textAlign: "center"`, `x` is the horizontal center of the
  text, not its left edge.
- `line`: `x`, `y`, `points` (an array of `[x, y]` pairs, at least 2; the first point is
  treated as `[0, 0]` and `x`/`y` become its actual position).
- `arrow`: same as `line`, plus optional `start`/`end`, each `{"id": "<alias or real id>"}`
  naming an existing (or same-batch) rectangle/diamond/ellipse to bind to, and optional
  `startArrowhead`/`endArrowhead` (e.g. `"arrow"`, `"triangle"`, `"bar"`). Omit either one
  to get napkin's default (none at the start, `"arrow"` at the end); send `null` to force
  no arrowhead at that end instead of the default. A bound end is nudged about half a pixel
  off the shape's edge; don't try to compensate for that yourself.
- `freedraw`: `x`, `y`, `points` (at least 1 pair).

Arrows do not follow the shapes they are bound to: moving or resizing a shape leaves its
bound arrows exactly where they were. After an `update` that moves or resizes a shape, send
a separate `update` for each of its arrows' `points`/`x`/`y` so they still meet the shape.

None of these accept a `label` except the three shape types above (arrows and lines don't
get napkin-managed labels — add a separate `text` element instead).

Every type also accepts `strokeColor`, `backgroundColor`, `fillStyle`
(`hachure`/`cross-hatch`/`solid`/`zigzag`), `strokeWidth`, `strokeStyle`
(`solid`/`dashed`/`dotted`), `roughness` and `opacity` (0-100), plus `groupIds`.

`update` takes `{"id": "...", "set": {...}}`; `set` accepts a subset of the same fields
depending on the element's type (a text label bound to a container can't take `x`/`y` —
move the container instead; a container's `set.text` edits/creates its label; lines and
arrows can't take `text` at all; an unrecognized ("raw") element can only have `x`/`y`
changed). `delete` takes `{"ids": [...]}`; deleting a container deletes its label and
unbinds any arrows pointing at it.

A batch is all-or-nothing: if any op fails, the whole batch is rejected and nothing
changes. `apply`'s error response looks like
`{"errors": [{"op": 0, "field": "id", "message": "no element \"does-not-exist\""}]}` —
`op` is the failing op's position in your `ops` array. On success, the response's
`warnings` array flags any label that doesn't fit its container, e.g.
`"r1: label needs 468x25 but the rectangle fits 50x30; make the rectangle larger or add
line breaks"` — napkin doesn't wrap text or resize the box for you, so widen it or add
`\n` and send an `update`.

### Example: a three-box flow with bound arrows

```json
{"ops": [
  {"op": "add", "type": "rectangle", "id": "start", "x": 40, "y": 40, "width": 160, "height": 70,
   "strokeColor": "#1971c2", "backgroundColor": "#a5d8ff", "fillStyle": "solid",
   "label": {"text": "Request"}},
  {"op": "add", "type": "rectangle", "id": "process", "x": 300, "y": 40, "width": 160, "height": 70,
   "strokeColor": "#1971c2", "backgroundColor": "#a5d8ff", "fillStyle": "solid",
   "label": {"text": "Process"}},
  {"op": "add", "type": "rectangle", "id": "done", "x": 560, "y": 40, "width": 160, "height": 70,
   "strokeColor": "#2f9e44", "backgroundColor": "#b2f2bb", "fillStyle": "solid",
   "label": {"text": "Response"}},
  {"op": "add", "type": "arrow", "x": 206, "y": 75, "points": [[0, 0], [88, 0]],
   "start": {"id": "start"}, "end": {"id": "process"}},
  {"op": "add", "type": "arrow", "x": 466, "y": 75, "points": [[0, 0], [88, 0]],
   "start": {"id": "process"}, "end": {"id": "done"}}
]}
```

### Example: a centered title and a dashed divider

```json
{"ops": [
  {"op": "add", "type": "text", "id": "heading", "x": 400, "y": 20, "text": "Pipeline overview",
   "fontSize": 28, "textAlign": "center"},
  {"op": "add", "type": "line", "x": 200, "y": 60, "points": [[0, 0], [400, 0]], "strokeStyle": "dashed"}
]}
```

### Example: add two boxes, restyle one, discard the other

```json
{"ops": [
  {"op": "add", "type": "rectangle", "id": "note", "x": 40, "y": 200, "width": 140, "height": 60,
   "label": {"text": "Draft"}},
  {"op": "add", "type": "rectangle", "id": "scratch", "x": 220, "y": 200, "width": 100, "height": 60},
  {"op": "update", "id": "note", "set": {"backgroundColor": "#ffec99", "fillStyle": "solid"}},
  {"op": "delete", "ids": ["scratch"]}
]}
```

## Layout conventions

Rough character width to plan box sizes before rendering (napkin doesn't wrap or resize
for you): about `0.55 x fontSize` per Latin character, about `1 x fontSize` for CJK.
Default `fontSize` is 20. Give a labeled box at least `text width + 40` for its own width,
and 60-80 height for a single line of text. Space sibling shapes 60-100 px apart. Keep
coordinates on multiples of 20. Lay out a main flow left-to-right or top-to-bottom. Start
an arrow 6-10 px outside the source shape's edge and end it 6-10 px before the target's
edge, and bind both ends with `start`/`end` rather than relying on the coordinates alone.

For color, pick from Excalidraw's own palette and keep `fillStyle: "solid"`: blue
`#1971c2` / `#a5d8ff`, green `#2f9e44` / `#b2f2bb`, red `#e03131` / `#ffc9c9`, yellow
`#f08c00` / `#ffec99`, purple `#6741d9` / `#d0bfff` (stroke / background). Don't use more
than three colors in one drawing.

## Pacing

Send one batch per meaningful step (all the boxes, then the arrows, then labels you're
adding after the fact) rather than one giant batch, so the user watches the drawing take
shape and can undo one step at a time. Keep a batch under about 15 elements.

## Check your work

After drawing, `napkin render --out /tmp/napkin.png` and `Read` it. Look for overlapping
shapes, text that doesn't fit its box (also check the `warnings` from `apply`), and arrows
that don't visually reach their targets. Fix what's wrong with another batch before
reporting back to the user.

## Errors

`napkin is not running; open it with SUPER+N` on stderr (exit 1) means there's no canvas
to talk to — ask the user to open one. A read-only canvas (its file failed to reload, or
similar) fails every `apply` with an error whose message is `the canvas is read-only:
<reason>`; tell the user why instead of retrying. An error message of `napkin is in the
middle of a drawing gesture; try again` means the user is actively dragging or drawing
something by hand; `apply` already waits for them to finish (up to 2 minutes) before it
runs, so on the rare race that still hits this, just resend the batch.
