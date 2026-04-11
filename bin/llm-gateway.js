#!/usr/bin/env node

const { spawnSync } = require('node:child_process')
const { resolveBinary } = require('../lib/resolve-binary')

const binPath = resolveBinary()
const result = spawnSync(binPath, process.argv.slice(2), { stdio: 'inherit' })

if (result.error) {
  console.error(result.error.message)
  process.exit(1)
}

process.exit(result.status ?? 0)
