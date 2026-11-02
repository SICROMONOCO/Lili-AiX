// Builds the WebAssembly providers and copies them to vendor/providers/ (bundled into the app).
// Usage: node scripts/build-providers.mjs

import { execFileSync } from "node:child_process"
import { copyFileSync, mkdirSync, readdirSync } from "node:fs"
import { join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
execFileSync("cargo", ["build", "--manifest-path", join(root, "providers", "Cargo.toml"), "--target", "wasm32-wasip2", "--release"], {
  stdio: "inherit",
  cwd: root,
})
const built = join(root, "providers", "target", "wasm32-wasip2", "release")
const out = join(root, "vendor", "providers")
mkdirSync(out, { recursive: true })
for (const file of readdirSync(built).filter((f) => f.startsWith("provider_") && f.endsWith(".wasm"))) {
  const name = file.replace(/^provider_/, "")
  copyFileSync(join(built, file), join(out, name))
  console.log(`vendor/providers/${name}`)
}
