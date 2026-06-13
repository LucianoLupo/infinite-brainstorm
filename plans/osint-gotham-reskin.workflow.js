export const meta = {
  name: 'osint-gotham-reskin',
  description: 'Plan -> audit -> implement the OSINT "Gotham-ops" visual reskin of infinite-brainstorm on feat/osint-gotham-reskin (colors+styles only, keep camera)',
  whenToUse: 'Run from /Users/lucianolupo/projects/infinite-brainstorm with feat/osint-gotham-reskin checked out. Reskins the canvas + DOM chrome to OSINT electric-blue Gotham. Stops before push; human merges.',
  phases: [
    { title: 'Plan', detail: 'Turn the plan doc into an atomic, file-targeted execution task list' },
    { title: 'Audit', detail: '4 parallel auditors (scope, deps/order, design-fidelity, regression) -> planDelta' },
    { title: 'Implement', detail: 'Sequential, build-gated: tokens+canvas -> DOM chrome -> polish, atomic commits' },
    { title: 'Verify', detail: 'fmt + clippy + test + wasm check + trunk build; final status' },
  ],
}

const REPO = '/Users/lucianolupo/projects/infinite-brainstorm'
const BRANCH = 'feat/osint-gotham-reskin'
const PLAN = 'plans/2026-06-12-osint-gotham-reskin.md'
const OSINT_REF = '/Users/lucianolupo/projects/osint-workbench/tauri-app/src' // App.css = :root token source. READ-ONLY.

// Canonical verify commands (frontend crate = infinite-brainstorm-ui at repo root; canvas.rs is wasm-only).
const VERIFY_FAST = 'cargo fmt --all && cargo clippy --all-targets 2>&1 | tail -25 && cargo check --target wasm32-unknown-unknown 2>&1 | tail -25 && cargo test 2>&1 | tail -25'
const VERIFY_FULL = 'cargo fmt --all --check; cargo clippy --all-targets 2>&1 | tail -30; cargo test 2>&1 | tail -20; cargo check --target wasm32-unknown-unknown 2>&1 | tail -20; trunk build 2>&1 | tail -30'

const CTX = `Repo: ${REPO} (work here, on branch ${BRANCH}, already checked out).
Plan (source of truth): ${REPO}/${PLAN} — read it fully first.
OSINT reference tokens: ${OSINT_REF}/App.css (:root), canvas.css, explorer/object-explorer.css. These are READ-ONLY — never edit anything under /Users/lucianolupo/projects/osint-workbench.
Decisions locked: full OSINT electric-blue (#4c90f0); canvas + DOM chrome in one pass; colors-and-styles ONLY; keep brainstorm's camera + grid pan/zoom behavior unchanged (recolor grid only; two-level grid & vignette are optional polish).
Frontend crate is infinite-brainstorm-ui (repo root Cargo.toml, holds src/canvas.rs); canvas.rs compiles only for wasm32.`

const TASKLIST_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['steps', 'gaps'],
  properties: {
    steps: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      required: ['id', 'title', 'files', 'summary', 'verify'],
      properties: {
        id: { type: 'string' }, title: { type: 'string' },
        files: { type: 'array', items: { type: 'string' } },
        summary: { type: 'string' }, verify: { type: 'string' },
      },
    } },
    gaps: { type: 'array', items: { type: 'string' } },
  },
}

const AUDIT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['dimension', 'findings'],
  properties: {
    dimension: { type: 'string' },
    findings: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      required: ['severity', 'note', 'fix'],
      properties: {
        severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'nit'] },
        note: { type: 'string' }, fix: { type: 'string' },
      },
    } },
  },
}

const DELTA_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['proceed', 'blockers', 'adjustments'],
  properties: {
    proceed: { type: 'boolean' },
    blockers: { type: 'array', items: { type: 'string' } },
    adjustments: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      required: ['step', 'change'],
      properties: { step: { type: 'string' }, change: { type: 'string' } },
    } },
  },
}

const IMPL_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['step', 'filesChanged', 'buildPassed', 'testsPassed', 'committed', 'notes'],
  properties: {
    step: { type: 'string' },
    filesChanged: { type: 'array', items: { type: 'string' } },
    buildPassed: { type: 'boolean' }, testsPassed: { type: 'boolean' },
    committed: { type: 'boolean' }, commitSha: { type: 'string' },
    notes: { type: 'string' },
  },
}

const VERIFY_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['fmt', 'clippy', 'tests', 'wasmCheck', 'trunkBuild', 'summary', 'issues'],
  properties: {
    fmt: { type: 'boolean' }, clippy: { type: 'boolean' }, tests: { type: 'boolean' },
    wasmCheck: { type: 'boolean' }, trunkBuild: { type: 'boolean' },
    summary: { type: 'string' }, issues: { type: 'array', items: { type: 'string' } },
  },
}

// ---------------------------------------------------------------------------
// Phase 1 — Plan: turn the prose plan into an atomic, file-targeted task list.
// ---------------------------------------------------------------------------
phase('Plan')
const tasklist = await agent(
  `${CTX}

Read the plan doc and the referenced source files (src/canvas.rs lines 12-34, src/app.rs HUD/status, src/components/*, styles.css, index.html) plus OSINT's App.css/canvas.css/object-explorer.css for the exact token values.
Produce an ATOMIC execution task list that an implementer can follow step by step. Keep the plan's Step 0/1/2/3 grouping. For each step give: id, title, the exact file(s) it touches, a 1-3 sentence summary of the concrete edits (which constant/inline-style maps to which token), and the verify command to run after it.
Do NOT write any code or edit any files — output the task list only. List any gaps/ambiguities you find in the plan in "gaps".`,
  { label: 'plan:finalize', phase: 'Plan', schema: TASKLIST_SCHEMA },
)
log(`Plan finalized: ${tasklist.steps.length} steps, ${tasklist.gaps.length} gaps flagged`)

// ---------------------------------------------------------------------------
// Phase 2 — Audit: 4 independent auditors, then synthesize a planDelta.
// Barrier is correct here: the synthesizer needs ALL findings at once.
// ---------------------------------------------------------------------------
phase('Audit')
const DIMS = [
  { key: 'scope-roi', brief: 'Scope & ROI: is anything out of scope (behavioral/feature change sneaking into a colors-only reskin)? Is the camera/grid behavior genuinely untouched? Any low-value churn to cut?' },
  { key: 'deps-order', brief: 'Dependencies & ordering: are the steps in a buildable order? Does Step 0 (tokens in styles.css) need to land before others? Any step that breaks the build if done before another?' },
  { key: 'design-fidelity', brief: 'Design fidelity: does every color/font/corner mapping faithfully reproduce OSINT App.css :root? Flag any wrong token value, missed hex, contrast problem, or place where green would leak through.' },
  { key: 'regression', brief: 'Regression risk: does the proportional-font switch break src text measurement/truncation (interaction.rs / state.rs tests)? Does deleting styles.css boilerplate break any referenced class? Will canvas.rs still compile for wasm32? What could silently break.' },
]
const audits = (await parallel(DIMS.map((d) => () =>
  agent(
    `${CTX}

You are the "${d.key}" auditor. Focus ONLY on: ${d.brief}
Read the plan doc, the task list below, and the actual source. Return concrete findings with severity (blocker/major/minor/nit), each with a specific fix. Be adversarial but precise — no vague concerns. If the plan is sound on your dimension, return few/no findings rather than inventing problems.

TASK LIST:
${JSON.stringify(tasklist.steps, null, 2)}

GAPS the planner flagged: ${JSON.stringify(tasklist.gaps)}`,
    { label: `audit:${d.key}`, phase: 'Audit', schema: AUDIT_SCHEMA },
  ),
))).filter(Boolean)

const allFindings = audits.flatMap((a) => (a.findings || []).map((f) => ({ ...f, dim: a.dimension })))
log(`Audit: ${allFindings.length} findings (${allFindings.filter((f) => f.severity === 'blocker').length} blocker, ${allFindings.filter((f) => f.severity === 'major').length} major)`)

const delta = await agent(
  `${CTX}

Synthesize the 4 audits into a single planDelta. Merge duplicates, drop nits that don't change the work, and decide adjustments to apply DURING implementation. Set proceed=false ONLY if there is a real blocker that must be resolved by the user before any code is written; otherwise proceed=true and fold fixes into "adjustments" (keyed by step id).

ALL FINDINGS:
${JSON.stringify(allFindings, null, 2)}`,
  { label: 'audit:synthesize', phase: 'Audit', schema: DELTA_SCHEMA },
)
log(`planDelta: proceed=${delta.proceed}, ${delta.blockers.length} blockers, ${delta.adjustments.length} adjustments`)

if (!delta.proceed) {
  log('BLOCKED — stopping before implementation. Returning the delta for user review.')
  return { stoppedAt: 'audit', tasklist, audits, delta }
}

// ---------------------------------------------------------------------------
// Phase 3 — Implement: SEQUENTIAL and build-gated. Edits land on the branch in
// the main working tree (no worktree — we WANT them on feat/osint-gotham-reskin).
// Each step verifies then commits atomically. Never pushes.
// ---------------------------------------------------------------------------
phase('Implement')
const adjustmentsFor = (ids) =>
  JSON.stringify(delta.adjustments.filter((a) => ids.some((id) => (a.step || '').toLowerCase().includes(id))), null, 2)

const stepDefs = [
  {
    id: 'tokens-canvas', ids: ['0', '1', 'token', 'canvas'],
    title: 'Step 0+1 — token system + canvas reskin',
    body: `Implement plan Step 0 (token system) AND Step 1 (canvas reskin) together:
- styles.css: replace the leftover Tauri/Leptos boilerplate with OSINT's exact App.css :root token block + base resets (html/body/#root bg, ::selection, thin flat scrollbars). styles.css becomes the DOM source of truth.
- src/canvas.rs lines 12-34: rewrite the color/font constants per the plan's mapping table (green -> Gotham). Comment each const with its "= var(--x)" equivalent so the two palettes can't drift. Recolor the grid (GRID_MINOR) WITHOUT changing pan/zoom behavior. Encode node-type via a 3px accent left-stripe (preferred) or the tinted-bg fallback. Split FONT into FONT_SANS (Inter) + FONT_MONO, OR keep a single mono FONT if the proportional switch risks the truncation math (the regression auditor's call).
- Two-level grid + vignette are OPTIONAL — only add if cheap and clean.`,
    verify: VERIFY_FAST,
    commit: 'feat(theme): Gotham token system + electric-blue canvas reskin',
  },
  {
    id: 'dom-chrome', ids: ['2', 'dom', 'chrome'],
    title: 'Step 2 — DOM chrome',
    body: `Implement plan Step 2 (DOM chrome). Add the small utility-class set to styles.css ported from OSINT canvas.css (.hud, .hud-btn, .pill, .pill-ready, .status-line, .modal, .modal-input, optional .canvas-vignette) with hover/focus accent+glow states. Convert the HUD buttons / status line from inline style= to class=. Recolor remaining inline styles in src/app.rs and src/components/* (error_banner, markdown_modal, node_editor, markdown_overlays, image_modal, search_overlay, minimap) to var(--token). Optional vignette overlay div over the canvas.`,
    verify: VERIFY_FAST,
    commit: 'feat(theme): re-skin DOM chrome (HUD/status/modals) to Gotham tokens',
  },
  {
    id: 'polish', ids: ['3', 'polish'],
    title: 'Step 3 — polish & parity',
    body: `Implement plan Step 3 (polish): square corners everywhere (inputs/buttons/modals radius 0), confirm ::selection accent + thin flat scrollbars are applied, soft shadows -> var(--panel-shadow). No behavioral change.`,
    verify: VERIFY_FAST,
    commit: 'style(theme): square corners, flat scrollbars, accent selection (Gotham parity)',
  },
]

const implResults = []
for (const s of stepDefs) {
  const r = await agent(
    `${CTX}

Implement: ${s.title}.
${s.body}

Apply these audit adjustments where relevant:
${adjustmentsFor(s.ids)}

Then VERIFY by running, in ${REPO}:
  ${s.verify}
Fix any compile/clippy/test failures you introduced and re-run until clean (work within your turn budget; if a failure is pre-existing and unrelated to your edits, note it and continue). Then stage and commit ONLY your changes with:
  git commit -m "${s.commit}"
Do NOT push. Do NOT touch osint-workbench. Report filesChanged, buildPassed (wasm check), testsPassed, committed + commitSha, and notes (incl. any decision like keeping mono FONT).`,
    { label: `impl:${s.id}`, phase: 'Implement', schema: IMPL_SCHEMA },
  )
  implResults.push(r)
  log(`${s.id}: build=${r?.buildPassed} tests=${r?.testsPassed} commit=${r?.committed ? (r.commitSha || 'yes') : 'NO'} — ${(r?.filesChanged || []).length} files`)
  if (r && r.buildPassed === false) {
    log(`STOP — ${s.id} left the build red; halting before later steps so the tree stays bisectable.`)
    break
  }
}

// ---------------------------------------------------------------------------
// Phase 4 — Verify: full CI-equivalent, independent of the implementers.
// ---------------------------------------------------------------------------
phase('Verify')
const verify = await agent(
  `${CTX}

Final independent verification (you did NOT write this code). In ${REPO} run:
  ${VERIFY_FULL}
Report each gate's pass/fail (fmt, clippy, tests, wasmCheck=cargo check wasm32, trunkBuild), a one-paragraph summary of the resulting visual state vs OSINT (from reading the diff + final constants, not screenshots), and any remaining issues. Do not fix anything — report only. Note that screenshot/visual tuning is a human follow-up.`,
  { label: 'verify:final', phase: 'Verify', schema: VERIFY_SCHEMA },
)
log(`VERIFY: fmt=${verify.fmt} clippy=${verify.clippy} tests=${verify.tests} wasm=${verify.wasmCheck} trunk=${verify.trunkBuild}`)

return {
  branch: BRANCH,
  steps: tasklist.steps.length,
  audit: { findings: allFindings.length, proceed: delta.proceed, adjustments: delta.adjustments.length },
  implemented: implResults.map((r) => ({ step: r?.step, build: r?.buildPassed, tests: r?.testsPassed, commit: r?.commitSha })),
  verify,
  nextStep: 'Human: launch `brainstorm <dir>`, screenshot vs OSINT, tune, then merge feat/osint-gotham-reskin.',
}
