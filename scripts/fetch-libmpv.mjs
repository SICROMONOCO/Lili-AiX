// Downloads the pinned LGPL libmpv build into vendor/libmpv/<platform>/.
//
// The app is closed source, so it may only ship an LGPL libmpv (no GPL parts),
// loaded dynamically so users can swap it. On Windows, zhongfly/mpv-winbuild
// publishes exactly that as `mpv-dev-lgpl-*`. On macOS nobody publishes an LGPL
// libmpv with Vulkan (MoltenVK), which player_mac.rs needs, so this runs
// scripts/build-libmpv-macos.sh, which builds one (and returns at once when it is up
// to date). Never use Homebrew's libmpv in its place: that one is GPL (x264, x265,
// rubberband).
//
// Usage: node scripts/fetch-libmpv.mjs

import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")

const PINS = {
    "win32-x64": {
        url: "https://github.com/zhongfly/mpv-winbuild/releases/download/2026-10-03-413ff0b1cd/mpv-dev-lgpl-x86_64-20261003-git-413ff0b1cd.7z",
        // Filled on first download; a mismatch afterwards fails the fetch.
        sha256: "12a9966bad239672c97276f01a9e625504e0b1d1fdcff95256066eeb8ffb1f21",
        dll: "libmpv-2.dll",
    },
}

// libmpv-2.dll imports vulkan-1.dll (the Vulkan loader) directly, so Windows refuses to load it
// when the PC has no loader or an old one missing an export it names (a user's PC had one
// without `vkCreateDisplayPlaneSurfaceKHR`: "Entry Point Not Found", and no app). Ship the
// Khronos loader beside it; `crates/mpv` makes Windows look in libmpv's folder first. The loader
// still finds the GPU drivers through the registry. Apache-2.0/MIT; its license ships with it.
const VULKAN_LOADER = {
    url: "https://sdk.lunarg.com/sdk/download/1.4.363.0/windows/VulkanRT-X64-1.4.363.0-Components.zip",
    sha256: "a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a",
    folder: "VulkanRT-X64-1.4.363.0-Components",
}

if (process.platform === "darwin") {
    execFileSync("bash", [join(root, "scripts", "build-libmpv-macos.sh")], { stdio: "inherit" })
    process.exit(0)
}

const key = `${process.platform}-${process.arch}`
if (key === "linux-x64") {
    // No pinned LGPL Linux archive yet: use the system libmpv (development/packaging on a
    // Linux host). This copies the distro library as-is; it does not establish LGPL
    // provenance for a release build (see docs/LINUX-PORT-AUDIT.md A02).
    const { copyFileSync } = await import("node:fs")
    const system = ["/usr/lib/x86_64-linux-gnu/libmpv.so.2", "/usr/lib64/libmpv.so.2", "/usr/lib/libmpv.so.2"].find((p) => existsSync(p))
    if (!system) {
        console.error("No system libmpv.so.2 found; install libmpv2/libmpv-dev first.")
        process.exit(1)
    }
    const outDir = join(root, "vendor", "libmpv", key)
    if (existsSync(join(outDir, "BUILD-INFO.txt"))) {
        console.log(`self-built libmpv present: ${join(outDir, "libmpv.so.2")} (see BUILD-INFO.txt)`)
        process.exit(0)
    }
    mkdirSync(outDir, { recursive: true })
    copyFileSync(system, join(outDir, "libmpv.so.2"))
    console.log(`libmpv copied from system: ${system} -> ${join(outDir, "libmpv.so.2")}`)
    process.exit(0)
}
const pin = PINS[key]
if (!pin) {
    console.error(`No libmpv pin for ${key}.`)
    process.exit(1)
}

// Windows 10+ ships bsdtar (libarchive), which reads 7z and zip. Call it by full path:
// Git Bash puts GNU tar first on PATH, and GNU tar reads "C:" as a remote host.
const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar"

/** Downloads `url` to `path`, checking it against `sha256`. */
async function download(url, sha256, path) {
    console.log(`Downloading ${url}`)
    const response = await fetch(url)
    if (!response.ok) throw new Error(`download failed: HTTP ${response.status}`)
    const bytes = Buffer.from(await response.arrayBuffer())
    const digest = createHash("sha256").update(bytes).digest("hex")
    if (sha256 && sha256 !== digest) {
        throw new Error(`sha256 mismatch: expected ${sha256}, got ${digest}`)
    }
    writeFileSync(path, bytes)
    console.log(`sha256 ${digest}`)
}

const outDir = join(root, "vendor", "libmpv", key)
mkdirSync(outDir, { recursive: true })

if (key === "win32-x64" && !existsSync(join(outDir, "vulkan-1.dll"))) {
    const archive = join(outDir, "vulkan-runtime.zip")
    await download(VULKAN_LOADER.url, VULKAN_LOADER.sha256, archive)
    const dll = `${VULKAN_LOADER.folder}/x64/vulkan-1.dll`
    const license = `${VULKAN_LOADER.folder}/VulkanRT-License.txt`
    execFileSync(tar, ["-xf", archive, "-C", outDir, dll, license], { stdio: "inherit" })
    renameSync(join(outDir, dll), join(outDir, "vulkan-1.dll"))
    renameSync(join(outDir, license), join(outDir, "VulkanRT-License.txt"))
    rmSync(join(outDir, VULKAN_LOADER.folder), { recursive: true })
    rmSync(archive)
    console.log(`Vulkan loader ready in ${outDir}`)
}

if (existsSync(join(outDir, pin.dll))) {
    console.log(`libmpv already present: ${join(outDir, pin.dll)}`)
    process.exit(0)
}

const archive = join(outDir, pin.url.endsWith(".7z") ? "libmpv.7z" : "libmpv.tar.gz")
await download(pin.url, pin.sha256, archive)
execFileSync(tar, ["-xf", archive, "-C", outDir], { stdio: "inherit" })
rmSync(archive)

if (pin.folder) {
    const inner = join(outDir, pin.folder)
    for (const name of readdirSync(inner)) renameSync(join(inner, name), join(outDir, name))
    rmSync(inner, { recursive: true })
}

if (!existsSync(join(outDir, pin.dll))) {
    throw new Error(`archive did not contain ${pin.dll}`)
}
const header = join(outDir, "include", "mpv", "client.h")
if (existsSync(header)) {
    const version = /MPV_CLIENT_API_VERSION MPV_MAKE_VERSION\((\d+), (\d+)\)/.exec(readFileSync(header, "utf8"))
    if (version) console.log(`client API ${version[1]}.${version[2]}`)
}
console.log(`libmpv ready in ${outDir}`)
