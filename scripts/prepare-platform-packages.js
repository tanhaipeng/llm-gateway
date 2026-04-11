#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')

const root = process.cwd()
const allowMissing = process.argv.includes('--allow-missing')

const targets = [
  {
    id: 'darwin-arm64',
    triple: 'aarch64-apple-darwin',
    binary: 'llm-gateway',
  },
  {
    id: 'darwin-x64',
    triple: 'x86_64-apple-darwin',
    binary: 'llm-gateway',
  },
  {
    id: 'linux-x64',
    triple: 'x86_64-unknown-linux-gnu',
    binary: 'llm-gateway',
  },
  {
    id: 'win32-x64',
    triple: 'x86_64-pc-windows-gnu',
    binary: 'llm-gateway.exe',
  },
]

const missing = []

for (const t of targets) {
  const dir = path.join(root, 'npm-bin', t.id)
  fs.mkdirSync(dir, { recursive: true })

  const primarySrc = path.join(root, 'target', t.triple, 'release', t.binary)
  const hostKey = `${process.platform}-${process.arch}`
  const fallbackSrc =
    hostKey === t.id ? path.join(root, 'target', 'release', t.binary) : null
  const src = fs.existsSync(primarySrc)
    ? primarySrc
    : fallbackSrc && fs.existsSync(fallbackSrc)
      ? fallbackSrc
      : primarySrc
  const dst = path.join(dir, t.binary)

  if (!fs.existsSync(src)) {
    missing.push(`${t.id}: ${src}`)
    continue
  }

  fs.copyFileSync(src, dst)
  fs.chmodSync(dst, 0o755)
}

if (missing.length > 0) {
  console.error('Missing binaries for targets:')
  for (const item of missing) {
    console.error(`- ${item}`)
  }
  if (!allowMissing) {
    process.exit(1)
  }
}

console.log('Prepared binaries under ./npm-bin')
