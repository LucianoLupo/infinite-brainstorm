export const meta = {
  name: 'ib-backlog',
  description: 'Plan the infinite-brainstorm backlog; implement + adversarially review the 2 safe items (launcher validate/query fix, headless `brainstorm export`) in isolated worktrees; produce audited design sketches for the large items. Worktree-isolated, never merges to main.',
  phases: [
    { title: 'Map' },
    { title: 'Plan' },
    { title: 'Audit' },
    { title: 'Implement' },
    { title: 'Review' },
    { title: 'Synthesize' },
  ],
}

const REPO = '/Users/lucianolupo/projects/infinite-brainstorm'

// Items to actually implement (scoped, low-risk, well-understood).
const EXECUTE = [
  {
    key: 'launcher-fix',
    title: 'Launcher validate/query passthrough',
    brief: `scripts/brainstorm treats $1 as a directory and always exec's the GUI, so the documented headless subcommands (brainstorm validate / query) never reach the binary (which DOES support them via clap in src-tauri/src/main.rs). Fix the wrapper so: if $1 is a known subcommand (validate|query|export|--help|-h|--version), pass all args straight to the binary from the current dir; otherwise keep the existing "treat $1 as target directory then launch GUI" behavior. No Rust changes expected — this is a bash-only fix in scripts/brainstorm. Keep it POSIX-ish and minimal.`,
  },
  {
    key: 'headless-export',
    title: 'brainstorm export (headless camera + SVG[/PNG])',
    brief: `Add a headless 'export' subcommand (sibling to validate/query in src-tauri/src/main.rs clap setup) that renders a board.json to an image WITHOUT opening the GUI — so an agent can position the camera and produce an image with no window. CLI shape: brainstorm export <board.json> --out <path.svg|path.png> [--fit | --region X,Y,W,H | --camera X,Y,ZOOM] [--nodes id,id | --group G] [--width N --height N]. Default --fit (fit all nodes with padding). Implementation: SVG-first, PURE RUST, emitting rects/text/edges/arrowheads/group boxes/labels from crates/brainstorm-types geometry, reusing the Gotham color constants and the same fit/bounds math the canvas uses. SCOPE GUARD: do NOT add a heavy rasterization dependency unless it integrates cleanly; if PNG via a pure-Rust SVG rasterizer (e.g. resvg/usvg) is clean, add it behind the .png extension, otherwise ship SVG-only and list PNG as a documented follow-up. This also closes the open "SVG/PDF export" backlog item. Add unit tests (deterministic SVG output for a small fixture board). Update README + CLAUDE.md + the skill (SKILL.md) docs and board export notes.`,
  },
]

// Large/ambiguous items: design sketches only — they need human decisions, not autonomous code.
const PLAN_ONLY = [
  'Multi-board / board switcher (multiple board files per directory + UI switcher)',
  'Semantic zoom (node summaries/collapsed labels when zoomed out, full content when zoomed in)',
  'Themes / light mode (the Gotham theme is currently the only one)',
  'Keyboard navigation (arrow-key traversal between connected nodes)',
  'Touch / tablet gestures (pan/zoom/select on touch devices)',
  'CRDT (Loro) real-time multi-client collaboration',
]

const MAP_SCHEMA = {
  type: 'object',
  required: ['cli', 'launcher', 'rendering', 'geometry', 'colors', 'notes'],
  properties: {
    cli: { type: 'string', description: 'How src-tauri/src/main.rs dispatches validate/query/GUI via clap (commands, arg parsing, where to add a subcommand).' },
    launcher: { type: 'string', description: 'Exact current behavior of scripts/brainstorm and why subcommands do not pass through.' },
    rendering: { type: 'string', description: 'How src/canvas.rs draws nodes/edges/arrowheads/group boxes/labels/text (fonts, wrapping, per-type colors).' },
    geometry: { type: 'string', description: 'Board/Node/Edge/Camera + fit/bounds/world<->screen helpers in crates/brainstorm-types/src/lib.rs (exact fn names).' },
    colors: { type: 'string', description: 'The Gotham color constants and where they live.' },
    notes: { type: 'string', description: 'Anything else a planner/implementer of export or the launcher fix must know (CLI test conventions, fixtures, gotchas).' },
  },
}

const SKETCH_SCHEMA = {
  type: 'object',
  required: ['item', 'approach', 'filesTouched', 'risks', 'effort', 'openQuestions'],
  properties: {
    item: { type: 'string' },
    approach: { type: 'string', description: '2-4 paragraph recommended approach grounded in the actual architecture.' },
    filesTouched: { type: 'array', items: { type: 'string' } },
    risks: { type: 'array', items: { type: 'string' } },
    effort: { type: 'string', enum: ['S', 'M', 'L', 'XL'] },
    openQuestions: { type: 'array', items: { type: 'string' }, description: 'Decisions the human must make before this can be built.' },
  },
}

const PLAN_SCHEMA = {
  type: 'object',
  required: ['title', 'steps', 'filesTouched', 'verify', 'backwardCompat'],
  properties: {
    title: { type: 'string' },
    steps: { type: 'array', items: { type: 'object', required: ['step', 'verify'], properties: { step: { type: 'string' }, verify: { type: 'string' } } } },
    filesTouched: { type: 'array', items: { type: 'string' } },
    verify: { type: 'string', description: 'Exact commands to prove it works (cargo test/clippy/build, brainstorm export smoke).' },
    backwardCompat: { type: 'string', description: 'How existing board.json / CLI behavior stays intact.' },
  },
}

const AUDIT_SCHEMA = {
  type: 'object',
  required: ['verdict', 'scopeIssues', 'dependencyIssues', 'designIssues', 'planDelta'],
  properties: {
    verdict: { type: 'string', enum: ['proceed', 'proceed-with-changes', 'rework'] },
    scopeIssues: { type: 'array', items: { type: 'string' } },
    dependencyIssues: { type: 'array', items: { type: 'string' } },
    designIssues: { type: 'array', items: { type: 'string' } },
    planDelta: { type: 'string', description: 'Concrete adjustments the implementer must apply before coding.' },
  },
}

const IMPL_SCHEMA = {
  type: 'object',
  required: ['branch', 'worktreePath', 'summary', 'diff', 'buildOk', 'testOk', 'commands', 'followUps'],
  properties: {
    branch: { type: 'string', description: 'Branch created inside the worktree (e.g. feat/headless-export).' },
    worktreePath: { type: 'string' },
    summary: { type: 'string' },
    diff: { type: 'string', description: 'Full unified diff of the committed change (git diff main...HEAD).' },
    buildOk: { type: 'boolean' },
    testOk: { type: 'boolean' },
    commands: { type: 'string', description: 'Exact build/test/clippy commands run and their pass/fail result.' },
    followUps: { type: 'array', items: { type: 'string' } },
  },
}

const REVIEW_SCHEMA = {
  type: 'object',
  required: ['item', 'mergeReady', 'findings'],
  properties: {
    item: { type: 'string' },
    mergeReady: { type: 'boolean' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['severity', 'title', 'detail', 'verified'],
        properties: {
          severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'nit'] },
          title: { type: 'string' },
          detail: { type: 'string' },
          verified: { type: 'boolean', description: 'Confirmed real after adversarially trying to refute it.' },
        },
      },
    },
  },
}

const ROADMAP_SCHEMA = {
  type: 'object',
  required: ['wrotePath', 'mergeReady', 'needsDecisions', 'summary'],
  properties: {
    wrotePath: { type: 'string' },
    mergeReady: { type: 'array', items: { type: 'string' }, description: 'Items implemented + reviewed clean, with branch names.' },
    needsDecisions: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
}

// ---- Phase 1: Map the relevant code once, for all downstream agents ----
phase('Map')
const map = await agent(
  `Read the infinite-brainstorm repo at ${REPO} (read-only). Produce a precise architecture map for agents who will (a) fix the launcher and (b) build a headless SVG/PNG exporter. Cover exactly: the clap CLI dispatch in src-tauri/src/main.rs (how validate/query/GUI are wired, where a new subcommand slots in, and the CLI test conventions under src-tauri/tests/); the scripts/brainstorm launcher (why subcommands don't pass through today); src/canvas.rs rendering (how nodes, edges, arrowheads, group bounding boxes, edge labels and text are drawn — fonts, text wrapping, per-type background colors); crates/brainstorm-types/src/lib.rs (Board/Node/Edge/Camera, plus any fit/bounds/world<->screen geometry helpers, with exact fn names); the Gotham color constants and where they live. Give exact file paths + function names.`,
  { phase: 'Map', label: 'map:repo', schema: MAP_SCHEMA }
)
log('Map complete — planning execute items + design-sketching the backlog')

// ---- Phases 2-5: execute items run plan -> audit -> implement(worktree) -> review, pipelined ----
const execPromise = pipeline(
  EXECUTE,
  // Stage 1: PLAN
  (item) => agent(
    `Write a concrete implementation plan for infinite-brainstorm item "${item.title}".\n\nTask: ${item.brief}\n\nArchitecture map: ${JSON.stringify(map)}\n\nProduce ordered steps each with an explicit verify check, the exact files touched, the exact verification commands, and how backward compatibility is preserved. Be specific to THIS codebase (real file paths, real fn names).`,
    { phase: 'Plan', label: `plan:${item.key}`, schema: PLAN_SCHEMA }
  ),
  // Stage 2: AUDIT (adversarial plan review before coding)
  (plan, item) => agent(
    `Adversarially audit this implementation plan for infinite-brainstorm item "${item.title}" BEFORE any code is written. Check scope creep, dependency risk (especially: is any new crate justified and minimal?), design consistency with the existing reducer/CLI/types architecture, and backward compatibility of board.json + existing CLI behavior. Plan: ${JSON.stringify(plan)}. Map: ${JSON.stringify(map)}. Return a verdict and a concrete planDelta the implementer MUST apply.`,
    { phase: 'Audit', label: `audit:${item.key}`, schema: AUDIT_SCHEMA }
  ).then((audit) => ({ item, plan, audit })),
  // Stage 3: IMPLEMENT in an isolated git worktree (parallel-safe; never touches main's working tree)
  (pa) => agent(
    `You are working in an isolated git worktree of infinite-brainstorm. Implement item "${pa.item.title}".\n\nTask: ${pa.item.brief}\n\nApproved plan: ${JSON.stringify(pa.plan)}\nApply this audit's planDelta first: ${JSON.stringify(pa.audit)}\n\nRules:\n- Create a branch named feat/${pa.item.key} in this worktree.\n- Make the change, keep it minimal and idiomatic to the surrounding code (match comment density + style).\n- Verify: run cargo fmt, cargo clippy, and the relevant tests (cargo test for workspace host tests; cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1 for backend). For the launcher fix, smoke-test the script's validate/query/export passthrough and the GUI-launch path arg parsing (without opening a window).\n- For export: add a deterministic unit test rendering a small fixture board to SVG. Honor the SCOPE GUARD on dependencies.\n- Update README.md, CLAUDE.md, and .claude/skills/infinite-brainstorm/SKILL.md if behavior/docs change.\n- Commit your work on the branch. DO NOT merge, DO NOT push, DO NOT touch main.\n- Return the branch, the worktree path, a full unified diff (git diff main...HEAD), build/test pass-fail with the exact commands, and any follow-ups.`,
    { phase: 'Implement', label: `impl:${pa.item.key}`, isolation: 'worktree', schema: IMPL_SCHEMA }
  ).then((impl) => ({ item: pa.item, impl })),
  // Stage 4: REVIEW (adversarial; verify each finding before reporting)
  (ii) => agent(
    `Adversarially review this implementation of infinite-brainstorm item "${ii.item.title}". Review the diff for correctness bugs, scope creep, convention violations, missing tests, and backward-compat breaks. For EACH finding, try to refute it before reporting — only mark verified:true if it survives. Decide mergeReady. Diff + build/test results: ${JSON.stringify(ii.impl)}`,
    { phase: 'Review', label: `review:${ii.item.key}`, schema: REVIEW_SCHEMA }
  ).then((review) => ({ item: ii.item.key, branch: ii.impl.branch, worktreePath: ii.impl.worktreePath, buildOk: ii.impl.buildOk, testOk: ii.impl.testOk, review }))
)

// Large items: cheap parallel design sketches (no code), run concurrently with the execute pipeline.
const sketchPromise = parallel(
  PLAN_ONLY.map((item) => () =>
    agent(
      `Design sketch (NO CODE) for infinite-brainstorm feature: "${item}". Given the architecture map: ${JSON.stringify(map)}. Produce a grounded recommended approach, files likely touched, key risks/unknowns, a rough effort (S/M/L/XL), and the open design questions a human must decide before this can be built.`,
      { phase: 'Plan', label: `sketch:${item.slice(0, 20)}`, schema: SKETCH_SCHEMA }
    )
  )
)

const [executed, sketches] = await Promise.all([execPromise, sketchPromise])
const execResults = executed.filter(Boolean)
const sketchResults = sketches.filter(Boolean)

// ---- Phase 6: Synthesize a human-facing roadmap doc ----
phase('Synthesize')
const roadmap = await agent(
  `Write a backlog roadmap to ${REPO}/plans/2026-06-14-backlog-roadmap.md and then return a structured summary.\n\nThe doc must contain:\n1. "Implemented (review before merge)" — for each execute item: branch name, worktree path, build/test status, and the verified review findings (call out any blockers/majors). These are on branches in worktrees and are NOT merged.\n2. "Design sketches (need your decisions)" — each plan-only item with approach, effort, risks, and the open questions.\n3. A recommended order and an explicit "Human decisions needed" checklist.\n\nExecute results: ${JSON.stringify(execResults)}\n\nDesign sketches: ${JSON.stringify(sketchResults)}`,
  { phase: 'Synthesize', label: 'synthesize:roadmap', schema: ROADMAP_SCHEMA }
)

return { roadmap, executed: execResults, sketches: sketchResults }
