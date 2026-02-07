---
name: infinite-brainstorm
description: Interact with the Infinite Brainstorm app - an agent-native infinite canvas. Create, organize, and connect ideas by editing board.json directly. Use when asked to brainstorm, create mind maps, organize ideas visually, or work with the brainstorm canvas.
---

# Infinite Brainstorm

An agent-native infinite canvas for human-AI collaboration. The core principle: **edit board.json directly** - changes sync to the canvas in <100ms.

## Quick Reference

**Location:** `./board.json` in the current working directory (or wherever the app was launched)

**Launch app:**
```bash
brainstorm              # Current directory
brainstorm /path/to/dir # Specific directory
```

## board.json Schema

```json
{
  "nodes": [
    {
      "id": "unique-uuid",
      "x": 0.0,
      "y": 0.0,
      "width": 200.0,
      "height": 100.0,
      "text": "Node content",
      "node_type": "text"
    }
  ],
  "edges": [
    {
      "id": "edge-uuid",
      "from_node": "source-node-id",
      "to_node": "target-node-id"
    }
  ]
}
```

**Edges are directed** — rendered as arrows from `from_node` to `to_node`. The line clips to node borders with an arrowhead at the target. Use edge direction to represent flows, dependencies, or hierarchies.

## Node Types

| Type | Use Case | Text Field |
|------|----------|------------|
| `text` | Default, simple text | Plain text |
| `idea` | Highlighted concepts (green) | Plain text |
| `note` | Annotations, comments (amber) | Plain text |
| `image` | Embedded images (blue) | Local path or URL |
| `md` | Markdown content (purple) | Markdown text |
| `link` | URL with preview (indigo) | URL or local `.md` path |

**Link nodes** with local `.md` file paths (absolute, `file://`, or `~/`) render as read-only markdown, enabling seamless Obsidian vault integration.

## Node Metadata (Optional)

All metadata fields are optional and backward-compatible. Omitted from JSON when empty/None.

```json
{
  "id": "uuid", "x": 0, "y": 0, "width": 200, "height": 100,
  "text": "A categorized node",
  "node_type": "idea",
  "color": "#ff6600",
  "tags": ["urgent", "pricing"],
  "status": "in-progress",
  "group": "cluster-a",
  "priority": 2
}
```

| Field | Type | Description |
|-------|------|-------------|
| `color` | `string?` | Custom border color (hex, e.g. `"#ff6600"`) |
| `tags` | `string[]` | Freeform tags for categorization |
| `status` | `string?` | Workflow status (e.g. `"todo"`, `"in-progress"`, `"done"`) |
| `group` | `string?` | Group ID for clustering related nodes |
| `priority` | `number?` | Priority level 1-5 (renders as P1-P5) |

**Visual rendering:**
- `color` overrides node border color
- `tags` render at bottom-left of node
- `status` renders as badge at top-right
- `priority` renders as P1-P5 next to type indicator

## Layout Algorithms

Grid is 50px. Default node size: 200x100. Min size: 50x30. Gap: 50px.

### Grid layout
Arrange N nodes in a rectangular grid. Good for lists, inventories, braindumps.
```
cols = ceil(sqrt(N))
x = (i % cols) * 250
y = floor(i / cols) * 150
```

### Tree layout (top-down hierarchy)
Root at top, children below. Each level gets its own row. Siblings spread evenly.
```
level_y = level * 200
# For each level, spread nodes across the total width:
x = (i - (count - 1) / 2) * 250   # centered on x=0
```
Best for org charts, decision trees, taxonomies. Connect parent → child with directed edges.

### Radial layout (mind map)
Center node at origin. Children in concentric rings.
```
angle = 2π * i / N           # even spacing
x = center_x + radius * cos(angle)
y = center_y + radius * sin(angle)
```
Ring 1 radius: 300. Ring 2: 550. Ring 3: 800. Increase by ~250 per level.

### Kanban columns
Columns by status. Cards stack vertically within each column.
```
col_x = col_index * 250      # column spacing
card_y = header_y + 110 + (row * 150)  # cards below header
```
Use `status` field to track column. Header nodes with `color` for column identity.

### Flowchart (top-to-bottom)
Sequential steps with decision branches. Main path vertical, branches horizontal.
```
step_y = step_index * 170     # vertical spacing
branch_x = ±250              # left/right for yes/no branches
```
Use directed edges for flow. Color decision nodes differently (e.g. amber).

### Timeline (left-to-right)
Phases flow horizontally. Details hang below.
```
phase_x = phase_index * 250   # horizontal spacing
detail_y = phase_y + 150      # details below phases
```
Use `status` + `color` to show progress (green=done, blue=active, gray=future).

### Clustering (reorganize existing nodes)
Group existing nodes by `group`, `tags`, or semantic similarity. Place each cluster in its own region.
```
cluster_origin_x = cluster_index * (cluster_width + 100)
# Within cluster, use grid layout offset from cluster origin
```
Set `color` per cluster for visual distinction.

## Board Templates

Ready-to-use board structures bundled as supporting files in this skill's directory.

| Template | File | Use Case |
|----------|------|----------|
| Mind Map | [templates/mind-map.json](templates/mind-map.json) | Central topic + radial branches + sub-ideas |
| Kanban | [templates/kanban.json](templates/kanban.json) | Status columns (To Do, In Progress, Review, Done) |
| Flowchart | [templates/flowchart.json](templates/flowchart.json) | Sequential steps with decision branches |
| SWOT | [templates/swot.json](templates/swot.json) | Strengths, Weaknesses, Opportunities, Threats |
| Pros/Cons | [templates/pros-cons.json](templates/pros-cons.json) | Two-column decision analysis |
| Timeline | [templates/timeline.json](templates/timeline.json) | Horizontal phases with milestones |

**Usage:**
```bash
# Read a template, customize the text content, write to board.json
cat ~/.claude/skills/infinite-brainstorm/templates/mind-map.json
# Then modify node texts and write to ./board.json
```

When creating boards from scratch, read the relevant template file, replace placeholder text with actual content, generate fresh UUIDs for all IDs, and write to `./board.json`. Extend by adding more nodes following the same layout pattern.

## Common Operations

### Read the board
```bash
cat ./board.json
```

### Create a mind map

1. Create central node at (0, 0)
2. Generate related ideas as child nodes
3. Position children radially or in tree layout
4. Connect with edges

Example - 5 nodes around a center:
```json
{
  "nodes": [
    {"id": "center", "x": 0, "y": 0, "width": 200, "height": 100, "text": "Main Topic", "node_type": "idea"},
    {"id": "n1", "x": 300, "y": -100, "width": 200, "height": 100, "text": "Subtopic 1", "node_type": "text"},
    {"id": "n2", "x": 300, "y": 100, "width": 200, "height": 100, "text": "Subtopic 2", "node_type": "text"},
    {"id": "n3", "x": -300, "y": -100, "width": 200, "height": 100, "text": "Subtopic 3", "node_type": "text"},
    {"id": "n4", "x": -300, "y": 100, "width": 200, "height": 100, "text": "Subtopic 4", "node_type": "text"}
  ],
  "edges": [
    {"id": "e1", "from_node": "center", "to_node": "n1"},
    {"id": "e2", "from_node": "center", "to_node": "n2"},
    {"id": "e3", "from_node": "center", "to_node": "n3"},
    {"id": "e4", "from_node": "center", "to_node": "n4"}
  ]
}
```

### Add nodes to existing board

1. Read current board.json
2. Parse JSON
3. Append new nodes with calculated positions (avoid overlaps)
4. Write back

### Organize/cluster nodes

1. Read all nodes
2. Group by `node_type`, `group`, `tags`, or semantic similarity
3. Recalculate positions for each cluster
4. Optionally set `color` per group for visual distinction
5. Write back

### Categorize with metadata

1. Read board
2. Analyze node texts
3. Set `tags`, `status`, `priority`, `group`, and `color` per node
4. Write back

### Connect related ideas

1. Read board
2. Analyze node texts for relationships
3. Add edges between related nodes
4. Write back

## UI Interactions (for reference)

| Action | Behavior |
|--------|----------|
| Double-click empty | Create new node |
| Double-click node | Edit text / open modal (image/md/link) |
| Drag node | Move selected nodes |
| Drag corner handle | Resize node (min 50x30) |
| Shift+drag from node | Create edge |
| Cmd/Ctrl+drag canvas | Box select nodes |
| Scroll wheel | Zoom (centered on cursor) |
| Cmd/Ctrl+V | Paste clipboard image at cursor |
| T | Cycle type on selected nodes |
| Delete/Backspace | Delete selected nodes or edge |
| Cmd/Ctrl+Z | Undo |
| Cmd/Ctrl+Shift+Z | Redo |
| Escape | Clear selection, cancel editing |

## Tips

- Generate UUIDs for new nodes/edges (use `uuidgen` or any UUID library)
- Preserve existing node IDs when updating - the UI tracks selection by ID
- Negative coordinates are valid - canvas is infinite
- Image nodes: set text to absolute path or URL
- Markdown nodes: full markdown syntax supported
- Link nodes: set text to URL (fetches OG metadata) or local `.md` path (renders markdown)
- Use `color` field to visually group related nodes
- Use `tags` for agent-driven filtering and categorization
- Use `status` + `priority` for kanban-style workflows

## Development

```bash
cargo tauri dev
```
