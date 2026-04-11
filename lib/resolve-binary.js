const path = require('node:path')
const fs = require('node:fs')

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`
  if (process.platform !== 'darwin') {
    throw new Error(`Unsupported platform: ${key}. This package only supports macOS.`)
  }

  const binaryPath = path.resolve(__dirname, '..', 'npm-bin', key, 'llm-gateway')
  if (!fs.existsSync(binaryPath)) {
    throw new Error(`Missing binary: ${binaryPath}. Run: npm run prepare:binaries`)
  }

  return binaryPath
}

module.exports = {
  resolveBinary,
}
