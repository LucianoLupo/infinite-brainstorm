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
      "to_node": "target-node-id",
      "label": "depends on"
    }
  ]
}
```

**Edges are directed** — rendered as arrows from `from_node` to `to_node`. The line clips to node borders with an arrowhead at the target. Use edge direction to represent flows, dependencies, or hierarchies.

**Edge `label` (optional)** — a short relationship string (e.g. `"depends on"`, `"blocks"`, `"related to"`) rendered at the edge midpoint in a background pill. Omit the field for an unlabeled edge.

**Board `version` (optional)** — an integer schema version. Omit it for the current format; a future, newer version still loads (with a console warning) so boards stay forward-compatible.

Full JSON Schema: [board.schema.json](board.schema.json).

## Node Types

| Type | Use Case | Text Field |
|------|----------|------------|
| `text` | Default, simple text | Plain text |
| `idea` | Highlighted concepts | Plain text |
| `note` | Annotations, comments | Plain text |
| `image` | Embedded images | Local path or URL |
| `md` | Markdown content | Markdown text |
| `link` | URL with preview | URL or local `.md` path |

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

## Software Architecture Diagrams

Standard diagramming on the canvas. Architecture diagrams are just boards with a **fixed visual language** so any reader (or agent) decodes them the same way. The canvas only has rectangles, six node types, per-node color, the `group` bounding box, and directed labeled edges — so shape semantics (cylinder=DB, diamond=decision, hexagon=core, lollipop=interface) are encoded with **text prefixes + color + node_type**, never lost. One diagram = one concern = one audience. Always include a title note and a legend (`md`) node.

### The Standard Visual Language (use across ALL templates)

| Role | node_type | color (border) | Convention |
|------|-----------|----------------|------------|
| Person / actor / user | `note` | `#f59e0b` (amber) | Actors are always amber notes |
| System / service / process **in focus** | `idea` | `#22c55e` (green) | The thing this diagram is about |
| External / third-party system | `text` | `#9ca3af` (gray) | Prefix `[External] ` |
| Database / data store / queue | `text` | `#06b6d4` (cyan) | Prefix `[DB] ` / `‹‹store›› ` / `[BUS] ` |
| Infra / gateway / LB / firewall / broker | `text` | `#a855f7` (violet) | Prefix `«gateway»` / `[LB]` / `[Firewall]` / `[BUS]` |
| Interface / contract / port | `link` | `#6366f1` (indigo) | Prefix `○ «interface» ` / `«port» ` |
| Class / entity / ADR / markdown body | `md` | `#a78bfa` (purple) | Renders compartments / fields / decision text |
| Decision / branch / fork / join (control) | `text` | `#e0b020` (gold) | Prefix `◇ ` / `▮ ` |
| Start node | `text` | `#22c55e` (green) | `● start` |
| End / final / error / compensation | `text`/`note` | `#ef4444`→`#c0392b` (red) | `◉ end` / `⊗` / `compensate:` |
| Domain event (event-storming / EDA) | `note` | `#f97316` (orange) | Past-tense name |
| Command (event-storming / CQRS) | `idea` | `#6366f1` (indigo) | Imperative name |

**Edges carry everything the canvas can't draw:** protocol (`calls [JSON/HTTPS]`), cardinality (`1 --- 0..N`), order (`3. validate`), relationship marker (`◁ extends`, `◆ owns 1..*`, `«include»`, `publishes OrderPlaced`). Arrow direction = dependency / call / flow direction. There are no dashed edges — encode async/dependency/realization **in the label** (`async:`, `⇢ depends`, `implements`).

**`group` = every boundary.** System boundary, package, layer, swimlane, trust zone, deployment node, bounded context, VPC/AZ/subnet — all are `group` values. Nesting is approximated with compound group names (`AWS us-east-1`, `AZ-a / public-subnet`). Put the boundary label in a header node at the box's top-left.

### Decision framework — which diagram?

Pick by **question + audience + abstraction level**. Zoom from outside in: Context → Container → Component → Code (C4) for *structure*; Sequence/Activity/State for *behavior*; ERD/DFD for *data*; Deployment/Cloud for *runtime topology*; ADR for *why*.

| What you're trying to communicate / the question | Recommended diagram(s) | Template file |
|---|---|---|
| "What is this system, who uses it, what does it depend on?" (big picture, any audience) | C4 System Context (L1) | `templates/c4-context.json` |
| "What are the major apps/datastores and how do they talk?" (engineers) | C4 Container (L2) | `templates/c4-container.json` |
| "How is this one complex container organized inside?" | C4 Component (L3) | `templates/c4-component.json` |
| "In exactly what order do the parts collaborate for this one feature?" | UML Sequence | `templates/uml-sequence.json` |
| "What are the types/entities, their fields, and how do they relate (is-a / has-a / multiplicity)?" | UML Class Diagram | `templates/uml-class.json` |
| "What states can this thing be in and what events move it?" | UML State Machine | `templates/uml-state-machine.json` |
| "What's the step-by-step workflow, where does it branch/parallelize, and who owns each step?" | UML Activity (swimlanes) | `templates/uml-activity.json` |
| "What's the relational schema — tables, keys, 1:N / M:N, optional vs mandatory?" | ERD (crow's foot) | `templates/erd-crows-foot.json` |
| "Where does data come from, what transforms it, where does it rest?" (scope/threat-modeling) | Data Flow Diagram | `templates/dfd-context.json` |
| "Which services exist, who calls whom (sync/async), who owns which DB?" | Microservices Service Map | `templates/microservices-service-map.json` |
| "Who publishes which events and who consumes them?" (async / pub-sub) | Event-Driven Flow | `templates/event-driven-flow.json` |
| "What's core domain vs replaceable plumbing; which deps point inward?" | Hexagonal (Ports & Adapters) | `templates/hexagonal-ports-adapters.json` |
| "How are responsibilities split top-to-bottom (UI→business→data)?" | Layered / N-tier (use Hexagonal or Activity-style stack) | `templates/hexagonal-ports-adapters.json` |
| "What runs where — instances, infra, networking — per environment?" | Deployment (C4 / UML) | `templates/deployment.json` |
| "Why did we decide X, and what did we trade away?" | ADR / Decision Log | `templates/adr-log.json` |
| "Have I covered every stakeholder / what's the doc structure?" | (meta) 4+1 views / arc42 index board — build from ADR + C4 templates | `templates/adr-log.json` + C4 set |
| "Which diagram should I even draw?" | This table → pick by question + audience + abstraction level | — |

| Audience | Reach for |
|----------|-----------|
| Everyone (exec, product, new hire) | C4 Context, C4 Container |
| Backend / OO engineers | UML Class, ERD, UML Component |
| Distributed-systems / platform | Microservices service map, Event-driven flow, Saga |
| QA / protocol reviewers | UML Sequence, State machine |
| Process / business stakeholders | Activity (swimlanes), Event Storming, DFD |
| Ops / SRE / security | Deployment, Cloud, Network, K8s |
| Future maintainers | ADR / decision log |

**Anti-patterns to refuse:** mixing abstraction levels in one board (containers next to classes next to VPCs); no legend; one mega-diagram doing every view's job; unlabeled edges (unknown protocol/direction); no title/audience/environment; showing everything instead of the relevant subset.

### Templates

Each template ships as `templates/<key>.json` with a realistic worked example. Read it, replace the domain text, regenerate UUIDs, write to `./board.json`, then `brainstorm validate`.

| Template | File | When to use |
|----------|------|-------------|
| C4 System Context (L1) | [templates/c4-context.json](templates/c4-context.json) | First diagram: system + its people + external deps. Audience: everyone |
| C4 Container (L2) | [templates/c4-container.json](templates/c4-container.json) | Apps + datastores + tech + protocols inside one system |
| C4 Component (L3) | [templates/c4-component.json](templates/c4-component.json) | Internal building blocks of one complex container |
| UML Sequence | [templates/uml-sequence.json](templates/uml-sequence.json) | Exact message order of one scenario (time flows down) |
| UML Class | [templates/uml-class.json](templates/uml-class.json) | Static OO/domain model: types, fields, inheritance, multiplicity |
| UML State Machine | [templates/uml-state-machine.json](templates/uml-state-machine.json) | Lifecycle: states + event-guarded transitions |
| UML Activity (swimlanes) | [templates/uml-activity.json](templates/uml-activity.json) | Workflow/process with branches, parallelism, ownership lanes |
| ERD (crow's foot) | [templates/erd-crows-foot.json](templates/erd-crows-foot.json) | Relational schema: tables, PK/FK, cardinality |
| Data Flow Diagram | [templates/dfd-context.json](templates/dfd-context.json) | Data-in-motion: processes, stores, flows, system boundary |
| Microservices Service Map | [templates/microservices-service-map.json](templates/microservices-service-map.json) | Service topology, sync/async coupling, DB-per-service |
| Event-Driven Flow | [templates/event-driven-flow.json](templates/event-driven-flow.json) | Producers → broker/topics → consumers; pub/sub |
| Hexagonal (Ports & Adapters) | [templates/hexagonal-ports-adapters.json](templates/hexagonal-ports-adapters.json) | Core domain isolated behind ports; driving vs driven adapters |
| Deployment (C4 / UML) | [templates/deployment.json](templates/deployment.json) | What runs where: nodes, instances, infra, per environment |
| ADR / Decision Log | [templates/adr-log.json](templates/adr-log.json) | Why the architecture is this way; status-colored decision chain |

**C4 (Context/Container/Component)** — zoom levels of one system. Notation: in-focus box highlighted, people above, external systems gray, every relationship a single unidirectional verb-labeled arrow. L1 omits tech; L2+ requires tech in node text `[Container: React SPA]` and protocol in every edge label `[JSON/HTTPS]`. Layout: 3 bands (people y=0 → focus y=260 → externals y=520) for L1; layered request flow (client→API→DB top-to-bottom) for L2/L3, all in-focus boxes sharing one `group` = the dashed system boundary. **C4 palette (deliberate exception to the green=in-focus rule):** the C4 templates use C4's canonical blues — system `#1168bd` → container `#438dd5` → component `#85bbf0` (lighter as you zoom in) — with person amber `#f59e0b`, external gray `#9ca3af`, datastore cyan `#06b6d4`. Keep these so the diagrams read as real C4 to any engineer.

**UML Sequence** — one column per participant, lifeline heads at the top, time flows **down**. Encode message kind in the label glyph: `→ call:`, `⇢ async:`, `⤺ return:`. `alt`/`loop`/`opt` fragments = a `group` named for the operator+guard wrapping the rows inside it. Never route an edge upward except an explicit return.

**UML Class** — one `md` rectangle per class: name line, `---`, attributes (`+/-/#` visibility), `---`, operations. Markers go in the edge label + direction: inheritance child→parent `◁ extends`; realization `◁ implements`; composition whole→part `◆ owns 1..*`; aggregation `◇ has 0..*`; association `1 — 0..*`; dependency `⇢ uses`. Always carry multiplicity. Base/abstract types on top so `extends` arrows point up; `group` boxes a package.

**State machine / Activity** — states/actions = `idea` green; decisions/forks/joins = gold `text` (`◇`/`▮`); start green / end red. Transition label = `trigger [guard] / effect`. Activity swimlanes = one `group` per actor lane; flow left→right within a lane.

**ERD** — entities = `idea` (strong) / cyan `text` (lookup) / amber `note` (weak), attributes inline (`PK `/`FK ` prefixes). Edge parent→child, label = verb + cardinality `places 1..*`; M:N resolved into a junction node. `group` = a schema/bounded context.

**DFD** — process = `idea` (number + verb name); external entity = amber `note` `«external»`; store = cyan `text` `‹‹store›› D1`; flow = edge labeled with the **data packet name** (never a verb). `group='system boundary'` wraps processes+stores; external entities sit outside.

**Microservices / EDA / Hexagonal** — service = `idea` green; its DB = cyan `[DB]` (database-per-service, never shared); broker/gateway = violet `[BUS]`/`«gateway»`. Sync vs async lives **only in the edge label** (`sync: REST GET /x` vs `async: publishes OrderPlaced`). Hexagonal: core = central `idea`, ports = indigo `«port»` nodes on its edges, driving adapters amber on the LEFT, driven adapters gray/cyan on the RIGHT, all dependency arrows pointing **inward** to the core.

**Deployment** — deployment node = `group` bounding box with an `idea` header `«executionEnvironment» [Tech]`; container instances inside = green `idea` `×N`; infra (LB/DNS/firewall) = red/violet `text`; DB = cyan. Edge labels carry protocol+port `Forwards to [HTTPS/443]`. One environment per board; tag every node `status='production'`.

**ADR** — one `md` node per decision titled `ADR-NNNN: <imperative>`, body = `## Status / ## Context / ## Decision / ## Consequences`. Border by status: accepted green, proposed amber, superseded/deprecated gray, rejected red. `supersedes` edge new→old; `group` by theme.

## Common Operations

### Read the board
```bash
cat ./board.json
```

### Validate a board (headless)
Catch duplicate ids, dangling edges, non-finite coordinates, and out-of-range priorities before they cause silent render bugs. Exits non-zero (with the offending id) when any structural error is found.
```bash
brainstorm validate              # validates ./board.json
brainstorm validate path/to/board.json
```

### Query a board (headless)
Inspect a board without opening the UI. Read-only; prints to stdout.
```bash
brainstorm query count           # node/edge counts
brainstorm query nodes           # id + type + first line of each node
brainstorm query edges           # id + from -> to (+ label)
brainstorm query node:<id>       # full detail for one node
brainstorm query type:idea       # node ids of a given type
brainstorm query tag:urgent      # node ids carrying a tag
brainstorm query status:done     # node ids with a status
brainstorm query group:cluster-a # node ids in a group
brainstorm query priority:1      # node ids at a priority
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
