// Builds src-tauri/THIRD-PARTY-NOTICES.txt: license texts of all third-party components that
// ship in the Windows build. Compiles nothing: it uses the dependency sources Cargo has
// already downloaded (`cargo fetch`) and the packages in node_modules.
// Run: npm run notices

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const TARGET = 'x86_64-pc-windows-msvc'
const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-_.].*)?$/i

const MIT_TEMPLATE = (holder) => `MIT License

Copyright (c) ${holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`

// These license texts contain no author names, so they can be taken from another package
const VERBATIM = new Set(['MPL-2.0', 'Apache-2.0', 'BSL-1.0'])

const meta = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--filter-platform', TARGET, '--manifest-path', join(root, 'src-tauri', 'Cargo.toml')],
    { maxBuffer: 256 * 1024 * 1024 }
  ).toString()
)

// Only what ships in the program: normal dependencies, without build- and dev-dependencies
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]))
const shipped = new Set()
const queue = [meta.resolve.root]
while (queue.length) {
  const id = queue.pop()
  if (shipped.has(id)) continue
  shipped.add(id)
  for (const dep of nodes.get(id).deps) {
    if (dep.dep_kinds.some((k) => k.kind === null)) queue.push(dep.pkg)
  }
}

function licenseTexts(dir) {
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((e) => ({ name: e.name, text: readFileSync(join(dir, e.name), 'utf8').replace(/\r\n/g, '\n').trim() }))
}

// "MIT OR Apache-2.0" and the like: the license is a choice, we take MIT, it is shorter
function pickMit(license, texts) {
  if (!/\bOR\b|\//.test(license ?? '') || !/\bMIT\b/.test(license)) return texts
  const mit = texts.filter((t) => /mit/i.test(t.name))
  return mit.length ? mit : texts
}

function chosenLicense(license) {
  return /\bOR\b|\//.test(license ?? '') && /\bMIT\b/.test(license) ? 'MIT' : license
}

const entries = []

for (const pkg of meta.packages) {
  if (!shipped.has(pkg.id) || pkg.source === null) continue // skip our own code
  entries.push({
    name: pkg.name,
    version: pkg.version,
    license: pkg.license ?? 'см. текст',
    url: pkg.repository,
    authors: pkg.authors,
    texts: pickMit(pkg.license, licenseTexts(dirname(pkg.manifest_path)))
  })
}

// From npm, only what is listed in dependencies ships in the program (the rest is needed only for the build)
const npmDeps = Object.keys(JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).dependencies ?? {})
for (const name of npmDeps) {
  const dir = join(root, 'node_modules', name)
  const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'))
  const url = typeof pkg.repository === 'string' ? pkg.repository : pkg.repository?.url
  entries.push({ name, version: pkg.version, license: pkg.license ?? 'см. текст', url, authors: [], texts: pickMit(pkg.license, licenseTexts(dir)) })
}

// Some crates.io packages have no license file. Take the text from a package in the same repository
// with the same license, then from any package for licenses without author names, then the MIT template
const unresolved = []
for (const e of entries.filter((e) => !e.texts.length)) {
  const license = chosenLicense(e.license)
  const donor =
    entries.find((d) => d.texts.length && d.url && d.url === e.url && chosenLicense(d.license) === license) ??
    (VERBATIM.has(license) ? entries.find((d) => d.texts.length && d.license === license) : undefined)
  if (donor) {
    e.texts = donor.texts
  } else if (license === 'MIT') {
    const holder = e.authors.length ? e.authors.map((a) => a.replace(/\s*<.*>/, '')).join(', ') : `the ${e.name} authors`
    e.texts = [{ name: 'LICENSE (MIT)', text: MIT_TEMPLATE(holder) }]
  } else {
    unresolved.push(`${e.name} ${e.version}: ${e.license}`)
  }
}

entries.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))

// Identical texts (e.g. Apache-2.0 shared by hundreds of packages) are printed once
const groups = new Map()
for (const e of entries) {
  const key = e.texts.map((t) => t.text).join('\n\n')
  if (!groups.has(key)) groups.set(key, { texts: e.texts, users: [] })
  groups.get(key).users.push(e)
}

const line = '='.repeat(78)
let out =
  'StopPic: сторонние компоненты / Third-party components\n\n' +
  'StopPic использует перечисленные ниже компоненты с открытым исходным кодом. Каждый из них\n' +
  'распространяется на условиях своей лицензии, текст которой приведён ниже. Лицензия\n' +
  'StopPic на эти компоненты не распространяется. Где компонент доступен на выбор\n' +
  'под несколькими лицензиями, используется лицензия MIT.\n\n' +
  'StopPic uses the open-source components listed below. Each is distributed under its own\n' +
  'license, the text of which is reproduced below. The StopPic license does not\n' +
  'apply to these components. Where a component is offered under a choice of licenses,\n' +
  'it is used under the MIT license.\n\n' +
  'Компоненты под MPL-2.0 используются без изменений; их исходный код доступен по ссылкам ниже.\n' +
  'Components under MPL-2.0 are used unmodified; their source code is available at the links below.\n\n' +
  'Список / List:\n'
for (const e of entries) out += `  ${e.name} ${e.version} (${e.license})${e.url ? ` ${e.url}` : ''}\n`

for (const g of groups.values()) {
  out += `\n${line}\nИспользуется в / Used by: ${g.users.map((e) => `${e.name} ${e.version}`).join(', ')}\n`
  for (const t of g.texts) out += `\n--- ${t.name} ---\n\n${t.text}\n`
}

writeFileSync(join(root, 'src-tauri', 'THIRD-PARTY-NOTICES.txt'), out)
console.log(`Компонентов: ${entries.length}, разных текстов лицензий: ${groups.size}`)
if (unresolved.length) {
  console.log(`\nНе найден текст лицензии (${unresolved.length}), добавьте вручную:`)
  for (const u of unresolved) console.log(`  ${u}`)
  process.exitCode = 1
}
