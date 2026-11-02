// Prepares the BoringSSL that wreq's TLS (btls-sys) builds against, in vendor/boringssl/: the
// copy btls-sys ships, with btls's own patches applied in btls's order, then ours from patches/.
// .cargo/config.toml points btls-sys at it (BORING_BSSL_SOURCE_PATH + ASSUME_PATCHED).
//
// Ours: patches/boringssl-mldsa-sigalgs.patch teaches libssl the TLS 1.3 ML-DSA signature
// schemes (0x0904-0x0906), which Chromium 154 offers first. Without them our ClientHello's
// signature list, and so its JA4, differs from the browser's.
//
// Usage: node scripts/prepare-boringssl.mjs   (then `cargo clean -p btls-sys` if it rebuilt)

import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { cpSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
const outDir = join(root, "vendor", "boringssl")
const ours = ["boringssl-mldsa-sigalgs.patch"].map((name) => join(root, "patches", name))

const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], { cwd: root, maxBuffer: 1 << 28 }).toString(),
)
const sys = metadata.packages.find((p) => p.name === "btls-sys")
if (!sys) throw new Error("btls-sys is not in the dependency graph")
const sysDir = dirname(sys.manifest_path)
// btls-sys's build script applies these, in this order, when it patches its own copy.
const theirs = ["boring-pq.patch", "boringssl.patch", "boringssl-loongarch.patch", "boringssl-windows.patch"].map((name) =>
    join(sysDir, "patches", name),
)

const stamp = createHash("sha256")
stamp.update(`btls-sys ${sys.version}\n`)
for (const patch of [...theirs, ...ours]) stamp.update(readFileSync(patch))
const digest = stamp.digest("hex")
const stampFile = join(outDir, ".anilili-prepared")
if (existsSync(stampFile) && readFileSync(stampFile, "utf8").trim() === digest) {
    console.log(`vendor/boringssl is current (btls-sys ${sys.version})`)
    process.exit(0)
}

console.log(`Copying BoringSSL from btls-sys ${sys.version}`)
rmSync(outDir, { recursive: true, force: true })
cpSync(join(sysDir, "deps", "boringssl"), outDir, { recursive: true })
// Its own repository: `git apply` inside this one would resolve paths from our root and skip them.
rmSync(join(outDir, ".git"), { recursive: true, force: true })
execFileSync("git", ["init", "-q"], { cwd: outDir })
for (const patch of [...theirs, ...ours]) {
    console.log(`applying ${patch}`)
    execFileSync("git", ["apply", "--whitespace=nowarn", patch], { cwd: outDir, stdio: "inherit" })
}
writeFileSync(stampFile, `${digest}\n`)
console.log("Prepared vendor/boringssl. If btls-sys was built before, run: cargo clean -p btls-sys")
