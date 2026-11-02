// Downloads the pinned Anime4K shaders (MIT) into vendor/shaders/.
// Usage: node scripts/fetch-shaders.mjs

import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
const URL = "https://github.com/bloc97/Anime4K/releases/download/v4.0.1/Anime4K_v4.0.zip"
const SHA256 = "139cd282086457c5adc79caf7b75b8b825091d71c9b54958c18745fea62d7ed7"
const outDir = join(root, "vendor", "shaders")

if (existsSync(join(outDir, "Anime4K_Upscale_CNN_x2_M.glsl"))) {
    console.log(`shaders already present in ${outDir}`)
    process.exit(0)
}
mkdirSync(outDir, { recursive: true })
console.log(`Downloading ${URL}`)
const response = await fetch(URL)
if (!response.ok) throw new Error(`download failed: HTTP ${response.status}`)
const bytes = Buffer.from(await response.arrayBuffer())
const digest = createHash("sha256").update(bytes).digest("hex")
if (SHA256 && SHA256 !== digest) throw new Error(`sha256 mismatch: expected ${SHA256}, got ${digest}`)
console.log(`sha256 ${digest}`)
const archive = join(outDir, "anime4k.zip")
writeFileSync(archive, bytes)
// Windows 10+ bsdtar reads zip; call it by path (Git Bash puts GNU tar first on PATH).
// GNU tar on Linux refuses zip archives, so Linux extracts with unzip instead.
const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar"
if (process.platform === "linux") {
    execFileSync("unzip", ["-o", "-q", archive, "-d", outDir], { stdio: "inherit" })
} else {
    execFileSync(tar, ["-xf", archive, "-C", outDir], { stdio: "inherit" })
}
rmSync(archive)
// The zip nests the shaders one level down on some releases; flatten.
for (const entry of readdirSync(outDir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
        for (const file of readdirSync(join(outDir, entry.name))) {
            execFileSync(process.platform === "win32" ? "cmd" : "mv", process.platform === "win32"
                ? ["/c", "move", "/y", join(outDir, entry.name, file), outDir]
                : [join(outDir, entry.name, file), outDir])
        }
        rmSync(join(outDir, entry.name), { recursive: true })
    }
}
console.log(`shaders ready in ${outDir}: ${readdirSync(outDir).filter((f) => f.endsWith(".glsl")).length} files`)
