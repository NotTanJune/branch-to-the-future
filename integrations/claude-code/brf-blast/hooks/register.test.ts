import { expect, test } from 'claude-code/testing'

const ROOT = '/repo'
const BRF = '/bin/brf'
const REX = '/bin/rex'

type Seen = { argv: string[][]; toasts: string[]; status: (string | undefined)[] }

function blastJson(level: 'low' | 'medium' | 'high', score: number): string {
  return JSON.stringify({
    score,
    level,
    changed: [{ path: 'src/a.ts', status: 'proposed' }],
    impacted: [{ path: 'src/b.ts', depth: 1 }],
    tests: [],
    untested_changes: ['src/a.ts'],
    sensitive: [],
    reasons: ['1 file imports src/a.ts'],
  })
}

type Opts = {
  // git toplevel (as git prints it) for the directories it owns
  tops?: string[]
  // real directories that exist; git -C on any other directory fails
  noBlast?: boolean
  splitThrows?: boolean
  // symlinked directory prefix -> real path (git prints the real toplevel)
  links?: Record<string, string>
  // directories that do not exist yet: git -C fails on them
  missing?: string[]
  cwd?: string
  rex?: boolean
}

// Stands in for git, which, brf and rex on the host.
function host(on: any, level: 'low' | 'medium' | 'high', score: number, o: Opts = {}): Seen {
  const rex = o.rex ?? true
  const tops = o.tops ?? [ROOT]
  const seen: Seen = { argv: [], toasts: [], status: [] }
  let pane: unknown = null
  on('session.cwd', () => ({ value: o.cwd ?? ROOT }))
  on('env.get', (_: unknown, e: { name: string }) => {
    if (e.name === 'HOME') return { value: '/home/me' }
    if (!rex) return { value: undefined }
    if (e.name === 'REX_SESSION') return { value: 'session:1' }
    if (e.name === 'REX_BLOCK') return { value: 'block:agent' }
    return { value: undefined }
  })
  on('process.run', (_: unknown, e: { argv: string[] }) => {
    seen.argv.push([...e.argv])
    const ok = (stdout: string) => ({ value: { exitCode: 0, stdout, stderr: '' } })
    const [cmd, ...rest] = e.argv
    if (cmd === 'git') {
      // git -C <dir> rev-parse --show-toplevel --show-prefix
      let dir = rest[1] ?? ''
      if (o.missing?.some(m => dir === m || dir.startsWith(m + '/'))) {
        return { value: { exitCode: 128, stdout: '', stderr: 'no such directory' } }
      }
      for (const [from, to] of Object.entries(o.links ?? {})) {
        if (dir === from || dir.startsWith(from + '/')) dir = to + dir.slice(from.length)
      }
      for (const top of tops) {
        if (dir === top) return ok(`${top}\n\n`)
        if (dir.startsWith(top + '/')) return ok(`${top}\n${dir.slice(top.length + 1)}/\n`)
      }
      return { value: { exitCode: 128, stdout: '', stderr: 'not a git repository' } }
    }
    if (cmd === '/usr/bin/which') return ok(rest[0] === 'brf' ? BRF : REX)
    if (cmd === BRF) {
      if (o.noBlast) return { value: { exitCode: 2, stdout: '', stderr: 'unrecognized subcommand' } }
      return ok(blastJson(level, score))
    }
    if (cmd === REX && rest[0] === 'split' && o.splitThrows) throw new Error('rex boom')
    if (cmd === REX && rest[0] === 'split') return ok(JSON.stringify({ block_ids: ['block:brf'] }))
    if (cmd === REX) return ok('{}')
    return { value: { exitCode: 1, stdout: '', stderr: 'unknown' } }
  })
  on('state.get', () => ({ value: { value: pane, version: 0 } }))
  on('state.set', (_: unknown, e: { value: unknown }) => {
    pane = e.value
    return { value: { isSet: true, version: 1 } }
  })
  on('ui.toast', (_: unknown, e: { text: string }) => {
    seen.toasts.push(e.text)
    return { value: undefined }
  })
  on('ui.status', (_: unknown, e: { text: string | undefined }) => {
    seen.status.push(e.text)
    return { value: undefined }
  })
  on('tool.call', () => ({ result: { ok: true } }))
  return seen
}

const blasts = (seen: Seen) => seen.argv.filter(a => a[0] === BRF && a[1] === 'blast' && a[2] === '--json')

test('a high radius edit toasts, opens the watch pane and notes the model', async ($, on) => {
  const seen = host(on, 'high', 8)
  const ran = await $.tool.call({ tool: 'Edit', file_path: `${ROOT}/src/a.ts`, old_string: 'a', new_string: 'b' })

  expect(seen.argv).toContainEqual([BRF, 'blast', '--json', ROOT, '--path', `${ROOT}/src/a.ts`])
  const split = seen.argv.find(a => a[0] === REX && a[1] === 'split')
  expect(split?.slice(-3)).toEqual([BRF, 'watch', ROOT])
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]).toContain('HIGH blast radius 8/10')
  expect(seen.status).toContain('brf high 8/10 · 1 impacted')
  expect((ran as { context?: string[] }).context?.[0]).toContain('Branch to the Future blast radius')
})

test('a low radius edit only sets the status line', async ($, on) => {
  const seen = host(on, 'low', 1)
  const ran = await $.tool.call({ tool: 'Write', file_path: `${ROOT}/src/a.ts`, content: 'x' })

  expect(seen.toasts.length).toBe(0)
  expect(seen.status).toContain('brf low 1/10 · 1 impacted')
  expect((ran as { context?: string[] }).context ?? []).toEqual([])
})

test('an edit outside the repo runs no blast scan', async ($, on) => {
  const seen = host(on, 'high', 9)
  await $.tool.call({ tool: 'Write', file_path: '/elsewhere/notes.md', content: 'x' })

  expect(seen.argv.some(a => a[0] === BRF)).toBe(false)
  expect(seen.toasts.length).toBe(0)
})

test('outside Rex, the edit is checked and no pane opens', async ($, on) => {
  const seen = host(on, 'high', 8, { rex: false })
  await $.tool.call({ tool: 'Edit', file_path: `${ROOT}/src/a.ts`, old_string: 'a', new_string: 'b' })

  expect(seen.argv.some(a => a[0] === REX)).toBe(false)
  expect(seen.toasts.length).toBe(1)
})

test('M1: a session that moves between repos blasts each one, and one started outside a repo recovers', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/m1a', '/m1b'] })
  await $.tool.call({ tool: 'Write', file_path: '/m1none/x.ts', content: 'x' })
  expect(blasts(seen).length).toBe(0)
  await $.tool.call({ tool: 'Write', file_path: '/m1a/src/x.ts', content: 'x' })
  await $.tool.call({ tool: 'Write', file_path: '/m1b/src/y.ts', content: 'x' })
  expect(blasts(seen).map(a => a.slice(3))).toEqual([
    ['/m1a', '--path', '/m1a/src/x.ts'],
    ['/m1b', '--path', '/m1b/src/y.ts'],
  ])
})

test('M2: dot segments, double slashes and relative paths are normalized', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/m2a', '/m2b'], cwd: '/m2a' })
  await $.tool.call({ tool: 'Write', file_path: '/m2a/../m2b/x.ts', content: 'x' })
  await $.tool.call({ tool: 'Write', file_path: '/m2a//src//y.ts', content: 'x' })
  await $.tool.call({ tool: 'Write', file_path: 'src/rel.ts', content: 'x' })
  expect(blasts(seen).map(a => a.slice(3))).toEqual([
    ['/m2b', '--path', '/m2b/x.ts'],
    ['/m2a', '--path', '/m2a/src/y.ts'],
    ['/m2a', '--path', '/m2a/src/rel.ts'],
  ])
})

test('M2: a symlinked path uses the toplevel git prints', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/real/m2c'], links: { '/link/m2c': '/real/m2c' } })
  await $.tool.call({ tool: 'Write', file_path: '/link/m2c/src/f.ts', content: 'x' })
  expect(blasts(seen).map(a => a.slice(3))).toEqual([['/real/m2c', '--path', '/real/m2c/src/f.ts']])
})

test('M2: a file in a directory that does not exist yet walks up to the nearest parent', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/m2d'], missing: ['/m2d/new'] })
  await $.tool.call({ tool: 'Write', file_path: '/m2d/new/deep/f.ts', content: 'x' })
  expect(blasts(seen).map(a => a.slice(3))).toEqual([['/m2d', '--path', '/m2d/new/deep/f.ts']])
  const gits = seen.argv.filter(a => a[0] === 'git').map(a => a[2])
  expect(gits).toEqual(['/m2d/new/deep', '/m2d/new', '/m2d'])
})

test('M3: an old brf without blast opens no pane and /brf status says so', async ($, on) => {
  const seen = host(on, 'high', 8, { tops: ['/m3'], noBlast: true, cwd: '/m3' })
  await $.tool.call({ tool: 'Write', file_path: '/m3/x.ts', content: 'x' })
  expect(seen.argv.some(a => a[0] === REX && a[1] === 'split')).toBe(false)
  const r: any = await ($.command as any).run({ command: 'brf', args: 'status' })
  expect(r.text).toContain('no blast subcommand')
})

test('M4/M5: a throwing split does not escape, and panes are keyed per root', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/m5a', '/m5b'] })
  await Promise.all([
    $.tool.call({ tool: 'Write', file_path: '/m5a/x.ts', content: 'x' }),
    $.tool.call({ tool: 'Write', file_path: '/m5b/y.ts', content: 'x' }),
  ])
  const splits = seen.argv.filter(a => a[0] === REX && a[1] === 'split')
  expect(splits.map(a => a[a.length - 1]).sort()).toEqual(['/m5a', '/m5b'])
})

test('M4: a throwing rex split never fails the edit', async ($, on) => {
  const seen = host(on, 'low', 1, { tops: ['/m4'], splitThrows: true })
  const ran = await $.tool.call({ tool: 'Write', file_path: '/m4/x.ts', content: 'x' })
  expect((ran as { deny?: string }).deny).toBeUndefined()
  expect(seen.status).toContain('brf low 1/10 · 1 impacted')
})
