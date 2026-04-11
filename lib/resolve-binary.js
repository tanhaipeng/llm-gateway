const path = require('node:path')
const fs = require('node:fs')

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`
  const supportedPlatforms = new Set(['darwin-arm64', 'darwin-x64', 'linux-x64', 'win32-x64'])
  if (!supportedPlatforms.has(key)) {
    throw new Error(
      `Unsupported platform: ${key}. This package supports darwin-arm64, darwin-x64, linux-x64 (glibc), and win32-x64.`
    )
  }

  const binaryName = process.platform === 'win32' ? 'llm-gateway.exe' : 'llm-gateway'
  const binaryPath = path.resolve(__dirname, '..', 'npm-bin', key, binaryName)
  if (!fs.existsSync(binaryPath)) {
    throw new Error(`Missing binary: ${binaryPath}. Run: npm run prepare:binaries`)
  }

  return binaryPath
}

module.exports = {
  resolveBinary,
}
