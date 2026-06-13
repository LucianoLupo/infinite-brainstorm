export const meta = {
  name: 'review-reskin',
  description: 'Multi-dimension review of the OSINT Gotham reskin diff (517c246..HEAD), each finding adversarially verified before it surfaces.',
  whenToUse: 'Post-merge review of the infinite-brainstorm Gotham reskin. Self-contained: repo path + diff base baked in.',
  phases: [
    { title: 'Review', detail: '4 dimensions review the reskin diff in parallel' },
    { title: 'Verify', detail: 'each finding is adversarially refuted; only confirmed ones survive' },
  ],
}

const REPO = '/Users/lucianolupo/projects/infinite-brainstorm'
const BASE = '517c246' // pre-reskin commit; HEAD = merged reskin (f7b6f7c)
const OSINT_REF = '/Users/lucianolupo/projects/osint-workbench/tauri-app/src/App.css' // token source of truth (READ-ONLY)

const CTX = `All work is in ${REPO} — every git/Read command MUST target it (use \`git -C ${REPO} ...\` and absolute paths under ${REPO}; do NOT assume the cwd is the repo).
The change under review is the "Gotham-ops" visual reskin: green-on-black -> OSINT electric-blue Gotham. It was meant to be COLORS-AND-STYLES ONLY with the camera/grid pan-zoom behavior unchanged.
See the diff with: git -C ${REPO} diff ${BASE}...HEAD
Key files: src/canvas.rs (color/font consts + draw), styles.css (:root token system, ported from ${OSINT_REF}), src/app.rs + src/components/* (inline styles -> utility classes + token recolor). plans/*.md and plans/*.workflow.js in the diff are docs/scripts — note issues only if egregious, they are not app code.`

const FINDINGS_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['findings'],
  properties: {
    findings: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      required: ['file', 'line', 'severity', 'title', 'detail', 'fix'],
      properties: {
        file: { type: 'string' },
        line: { type: 'string', description: 'line or range, e.g. "16" or "284-302"' },
        severity: { type: 'string', enum: ['blocker', 'high', 'medium', 'low'] },
        title: { type: 'string' },
        detail: { type: 'string' },
        fix: { type: 'string' },
      },
    } },
  },
}

const VERDICT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['isReal', 'reason'],
  properties: {
    isReal: { type: 'boolean', description: 'true ONLY if concretely confirmed in the actual code and reachable' },
    reason: { type: 'string' },
  },
}

const DIMENSIONS = [
  { key: 'correctness', brief: 'Bugs introduced by the recolor: a constant mapped to the wrong token/value; an rgba/hex string canvas2D set_*_style_str cannot parse; the grid recolor accidentally changing pan/zoom math (must be behavior-identical); the FONT split (FONT_SANS/FONT_MONO) altering measure_text/wrap/truncation; an inline-style -> class conversion that dropped a needed property (e.g. HUD anchoring, z-index, pointer-events); node-type bg fallback collisions; any var(--x) referenced in inline style with no matching :root token (renders invalid/initial).' },
  { key: 'architecture', brief: 'Token-system integrity: do the src/canvas.rs consts faithfully MIRROR styles.css :root (no drift; each pinned with `= var(--x)`)? Is styles.css the single DOM source of truth? Did removing the old Tauri/Leptos boilerplate (.logo/.container/.row/#greet-input/prefers-color-scheme) break anything still referenced in markup? Any layering/separation regression.' },
  { key: 'simplicity', brief: 'Dead or redundant code from the reskin: unused consts (e.g. a GRID_MAJOR introduced but never drawn => dead_code), leftover commented hex, duplicated style declarations that should be a class/token, over-engineering. Flag only reskin-introduced cruft, not pre-existing.' },
  { key: 'conventions', brief: 'Naming + codebase conventions + DOC DRIFT. Specifically: this repo CLAUDE.md and .claude/skills/infinite-brainstorm/SKILL.md still document the OLD GREEN node-type colors (e.g. "idea (green)", "text -> Dark green #040804") which now mismatch the Gotham palette — confirm and flag. Also: `= var(--x)` comment discipline, the node-type accent-stripe deferral being honestly documented, any green hex still lurking in src/.' },
]

phase('Review')
const results = await pipeline(
  DIMENSIONS,
  d => agent(
    [
      CTX, '',
      `Review the reskin diff for the "${d.key}" dimension ONLY.`,
      d.brief, '',
      `Get the diff: git -C ${REPO} diff ${BASE}...HEAD — review ONLY changed lines, but Read surrounding files (and ${OSINT_REF} for token fidelity) for context.`,
      'Report concrete findings only — no style nits unless they cause real harm. If the dimension is clean, return an empty findings array.',
    ].join('\n'),
    { label: `review:${d.key}`, phase: 'Review', schema: FINDINGS_SCHEMA },
  ),
  (review, d) => parallel(((review && review.findings) || []).map((f) => () =>
    agent(
      [
        CTX, '',
        'Adversarially verify this code-review finding. Your job is to REFUTE it.',
        `Read the actual code at ${REPO}/${f.file} around line ${f.line} and trace whether the problem is real and reachable.`,
        'Default to isReal=false if you cannot concretely confirm it. A plausible-but-unproven concern is NOT real.',
        '',
        `Finding: ${JSON.stringify(f)}`,
      ].join('\n'),
      { label: `verify:${d.key}`, phase: 'Verify', schema: VERDICT_SCHEMA },
    ).then((v) => ({ ...f, dimension: d.key, verdict: v }))
  )),
)

const all = results.flat().filter(Boolean)
const confirmed = all.filter((f) => f.verdict && f.verdict.isReal)
const dropped = all.length - confirmed.length
const order = { blocker: 0, high: 1, medium: 2, low: 3 }
confirmed.sort((a, b) => (order[a.severity] ?? 9) - (order[b.severity] ?? 9))
log(`reviewed ${all.length} raw findings -> ${confirmed.length} confirmed, ${dropped} dropped by adversarial verification`)

return {
  base: BASE,
  confirmed: confirmed.map((f) => ({ severity: f.severity, dimension: f.dimension, file: f.file, line: f.line, title: f.title, detail: f.detail, fix: f.fix })),
  droppedCount: dropped,
  rawCount: all.length,
}
