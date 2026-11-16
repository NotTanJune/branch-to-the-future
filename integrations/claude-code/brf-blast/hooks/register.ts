import type { EngineInterface, Register, ToolCallResult } from 'claude-code'

// Branch to the Future beside the agent.
// - Every Edit, Write or NotebookEdit inside a git repo runs `brf blast --json`
//   for the proposed file before the permission prompt, sets the status line,
//   and on a high radius raises a toast and adds a note the model reads.
// - Inside Rex, the first such edit opens a split running `brf watch <repo>`,
//   which reruns the scan every time the working tree changes.
// - `/brf` opens the pane now; `/brf close` closes it; `/brf status` reports.

type Blast = {
  score: number
  level: 'low' | 'medium' | 'high'
  changed: { path: string; status: string }[]
  impacted: { path: string; depth: number }[]
  tests: string[]
  untested_changes: string[]
  sensitive: string[]
  reasons: string[]
}

type Rex = { bin: string; session: string; block: string }
type Tools = { brf: string; rex: Rex | null }
type Ctx = { root: string; brf: string; rex: Rex | null }
type Located = { root: string; prefix: string }

const PANE = { plugin: 'brf-blast', key: 'pane' } as const

// Only successes are cached: a failed lookup must never stick for the session.
let toolsCache: Tools | null = null
const rootCache = new Map<string, Located>()
const opening = new Map<string, Promise<string | null>>()
const blastSupport = new Map<string, boolean>()

async function which($: EngineInterface, name: string): Promise<string | null> {
  const r = await $.process.run(['/usr/bin/which', name], { timeoutMs: 3000 })
  const path = r.stdout.trim()
  return r.exitCode === 0 && path ? path : null
}

async function tools($: EngineInterface): Promise<Tools | null> {
  if (toolsCache) return toolsCache
  const home = await $.env.get('HOME')
  let brf = await which($, 'brf').catch(() => null)
  if (!brf && home) {
    const candidate = `${home}/.cargo/bin/brf`
    const probe = await $.process.run([candidate, '--help'], { timeoutMs: 3000 }).catch(() => null)
    if (probe && probe.exitCode === 0) brf = candidate
  }
  if (!brf) return null
  const session = await $.env.get('REX_SESSION')
  const block = await $.env.get('REX_BLOCK')
  const rexBin = session && block ? ((await which($, 'rex').catch(() => null)) ?? '/opt/homebrew/bin/rex') : null
  toolsCache = { brf, rex: rexBin && session && block ? { bin: rexBin, session, block } : null }
  return toolsCache
}

// Collapses '.', '..' and repeated slashes. Input must be absolute.
function normalize(path: string): string {
  const out: string[] = []
  for (const part of path.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') out.pop()
    else out.push(part)
  }
  return '/' + out.join('/')
}

function dirname(path: string): string {
  const i = path.lastIndexOf('/')
  return i <= 0 ? '/' : path.slice(0, i)
}

async function toplevel($: EngineInterface, dir: string): Promise<Located | null> {
  const hit = rootCache.get(dir)
  if (hit) return hit
  const r = await $.process
    .run(['git', '-C', dir, 'rev-parse', '--show-toplevel', '--show-prefix'], { timeoutMs: 5000 })
    .catch(() => null)
  if (!r || r.exitCode !== 0) return null
  const [root, prefix = ''] = r.stdout.split('\n').map(l => l.trim())
  if (!root) return null
  const found = { root, prefix }
  rootCache.set(dir, found)
  return found
}

// The repo that owns a file, and the file's path inside it as git sees it, so a
// symlinked cwd or a not-yet-created directory cannot make the two disagree.
async function locate($: EngineInterface, raw: string): Promise<{ root: string; file: string } | null> {
  let abs = raw
  if (!abs.startsWith('/')) {
    const cwd = await $.session.cwd().catch(() => null)
    if (!cwd) return null
    abs = `${cwd}/${abs}`
  }
  abs = normalize(abs)
  if (abs === '/') return null
  const tail = [abs.slice(abs.lastIndexOf('/') + 1)]
  let dir = dirname(abs)
  for (let depth = 0; depth < 64; depth++) {
    const found = await toplevel($, dir)
    if (found) return { root: found.root, file: `${found.root}/${found.prefix}${tail.join('/')}` }
    if (dir === '/') return null
    tail.unshift(dir.slice(dir.lastIndexOf('/') + 1))
    dir = dirname(dir)
  }
  return null
}

async function context($: EngineInterface, root: string): Promise<Ctx | null> {
  const t = await tools($)
  return t ? { root, brf: t.brf, rex: t.rex } : null
}

async function supportsBlast($: EngineInterface, brf: string): Promise<boolean> {
  const known = blastSupport.get(brf)
  if (known !== undefined) return known
  const r = await $.process.run([brf, 'blast', '--help'], { timeoutMs: 5000 }).catch(() => null)
  if (!r) return true // could not probe: do not claim the subcommand is missing
  const ok = r.exitCode === 0
  blastSupport.set(brf, ok)
  return ok
}

async function blast($: EngineInterface, ctx: Ctx, path?: string): Promise<Blast | null> {
  const argv = [ctx.brf, 'blast', '--json', ctx.root]
  if (path) argv.push('--path', path)
  const r = await $.process.run(argv, { cwd: ctx.root, timeoutMs: 15000 }).catch(() => null)
  if (!r || r.exitCode !== 0) {
    if (r) await supportsBlast($, ctx.brf)
    return null
  }
  try {
    return JSON.parse(r.stdout) as Blast
  } catch {
    return null
  }
}

async function paneAlive($: EngineInterface, rex: Rex, block: string): Promise<boolean> {
  const r = await $.process
    .run([rex.bin, 'block', 'inspect', block, '-s', rex.session, '--json'], { timeoutMs: 5000 })
    .catch(() => null)
  return !!r && r.exitCode === 0
}

async function openPane($: EngineInterface, ctx: Ctx): Promise<string | null> {
  const rex = ctx.rex
  if (!rex) return null
  const { value } = await $.state.get(PANE)
  if (value && value.root === ctx.root && (await paneAlive($, rex, value.block))) return value.block
  const r = await $.process
    .run(
    [
      rex.bin, 'split', '-s', rex.session, '-b', rex.block,
      '--split=right', '--ratio', '38', '--focus=false',
      '--label', 'brf', '-c', ctx.root, '--json', '--',
      ctx.brf, 'watch', ctx.root,
    ],
    { timeoutMs: 10000 },
    )
    .catch(() => null)
  if (!r || r.exitCode !== 0) return null
  let block: string | undefined
  try {
    block = (JSON.parse(r.stdout) as { block_ids?: string[] }).block_ids?.[0]
  } catch {
    block = undefined
  }
  if (!block) return null
  await $.state.set(PANE, { block, root: ctx.root })
  return block
}

// One opening per repo at a time: parallel edits must not stack several panes.
function ensurePane($: EngineInterface, ctx: Ctx): Promise<string | null> {
  let pending = opening.get(ctx.root)
  if (!pending) {
    pending = openPane($, ctx)
      .catch(() => null)
      .finally(() => opening.delete(ctx.root))
    opening.set(ctx.root, pending)
  }
  return pending
}

async function closePane($: EngineInterface, ctx: Ctx): Promise<boolean> {
  const { value } = await $.state.get(PANE)
  if (!value || !ctx.rex) return false
  await $.process
    .run([ctx.rex.bin, 'block', 'close', value.block, '-s', ctx.rex.session], { timeoutMs: 5000 })
    .catch(() => null)
  await $.state.set(PANE, null)
  return true
}

function statusText(b: Blast): string {
  return `brf ${b.level} ${b.score}/10 · ${b.impacted.length} impacted`
}

function summary(b: Blast, max = 4): string {
  const reasons = b.reasons.slice(0, max).map(r => `- ${r}`).join('\n')
  return `${b.level.toUpperCase()} ${b.score}/10, ${b.changed.length} changed, ${b.impacted.length} impacted, ${b.tests.length} related tests\n${reasons}`
}

function relative(root: string, path: string): string {
  return path.startsWith(root + '/') ? path.slice(root.length + 1) : path
}

// Runs the blast check for a proposed edit, then the edit itself (permission
// prompt included). Informs only: never denies, and fails open.
async function guardEdit<R extends ToolCallResult>(
  $: EngineInterface,
  raw: string,
  run: () => Promise<R>,
): Promise<R> {
  let ctx: Ctx | null = null
  let file = raw
  try {
    const where = await locate($, raw)
    ctx = where ? await context($, where.root) : null
    if (where) file = where.file
  } catch {
    ctx = null
  }
  if (!ctx) return run()

  const report = await blast($, ctx, file)
  if (!report) return run()
  void ensurePane($, ctx).catch(() => null)

  $.ui.status(statusText(report))
  const isHigh = report.level === 'high'
  if (isHigh) {
    $.ui.toast(`brf: HIGH blast radius ${report.score}/10 for ${relative(ctx.root, file)}: ${report.reasons[0] ?? ''}`)
  }

  const ran = await run()
  if (!isHigh || ran.deny !== undefined || ran.isError) return ran
  const note =
    `Branch to the Future blast radius for this edit to ${relative(ctx.root, file)}:\n${summary(report)}\n` +
    'Before finishing, check the impacted files and run the related tests.'
  return { ...ran, context: [...(ran.context ?? []), note] }
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'brf',
      description: 'Branch to the Future: open the brf watch pane (args: close, status)',
    })
    return next(e)
  })

  on('command.run', { command: 'brf' }, async ($, e) => {
    const cwd = await $.session.cwd().catch(() => null)
    const where = cwd ? await locate($, `${cwd}/.brf-probe`).catch(() => null) : null
    const ctx = where ? await context($, where.root).catch(() => null) : null
    if (!ctx) {
      return { text: 'brf: not in a git repo, or the brf binary is not installed (cargo install --path .).' }
    }
    const arg = e.args.trim()
    if (arg === 'close') {
      return { text: (await closePane($, ctx)) ? 'brf: pane closed.' : 'brf: no pane open.' }
    }
    if (arg === 'status') {
      const report = await blast($, ctx)
      if (!report) {
        if (!(await supportsBlast($, ctx.brf))) {
          return { text: `brf: the installed brf (${ctx.brf}) has no blast subcommand. Reinstall it with cargo install --path . in the branch-to-the-future repo.` }
        }
        return { text: 'brf: blast scan failed.' }
      }
      $.ui.status(statusText(report))
      return { text: `brf: ${summary(report, 8)}` }
    }
    if (!ctx.rex) {
      return { text: `brf: not inside Rex. Run this in a split beside the agent:\n  brf watch ${ctx.root}` }
    }
    const block = await ensurePane($, ctx)
    return { text: block ? `brf: watching ${ctx.root} in pane ${block}.` : 'brf: could not open the Rex pane.' }
  })

  on('tool.call', { tool: 'Edit' }, ($, e, next) => guardEdit($, e.file_path, () => next(e)))
    .catch(($, e, next) => next(e))
  on('tool.call', { tool: 'Write' }, ($, e, next) => guardEdit($, e.file_path, () => next(e)))
    .catch(($, e, next) => next(e))
  on('tool.call', { tool: 'NotebookEdit' }, ($, e, next) => guardEdit($, e.notebook_path, () => next(e)))
    .catch(($, e, next) => next(e))
}
