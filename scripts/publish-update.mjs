// Publishes a desktop release to the signed update channel. Run by a person, never by CI.
//
//   node scripts/publish-update.mjs --key <file with the hex private key> --version 0.2.0 \
//     [--windows <.exe URL>] [--macos <.dmg URL>] [--linux <.AppImage or .deb URL>] \n//     [--changelog-file <text file>] [--relays]
// Release notes: one concise user-facing change per line, prefixed with "- ".
// --changelog "…" is also accepted; use a file to preserve multiline notes.
//
// Each installer is downloaded from its URL to measure and hash it, so what is signed is exactly
// what users will fetch, and either machine can publish both platforms. Upload the installers
// first. A platform left out gets no update from this release.
//
// Writes target/release/desktop.json: the signed Nostr event (kind 30078, d-tag
// anilili-desktop-update) to upload to the mirror as latest/desktop.json. With --relays it is also
// published to the relays the Android channel uses. The private key is the project's Nostr update
// key (../anilili-private/); the app only accepts events signed by its public key.

import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

import { schnorr } from "@noble/curves/secp256k1.js"

const PUBKEY = "f1abd1ac685aed1f5145271283cd9418bf13aa92ed26399df191ae0c08b22207"
const KIND = 30078
const D_TAG = "anilili-desktop-update"
const RELAYS = ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.nostr.band", "wss://relay.primal.net"]

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]?.startsWith("--") || all[i + 1] === undefined ? true : all[i + 1]])
    return pairs
  }, []),
)
// Provider modules: --providers <json file> with [{"id","version","url","file"}] (file = local .wasm).
const providersMode = typeof args.providers === "string"
if (args["changelog-file"] && args.changelog) throw new Error("use --changelog-file or --changelog, not both")
for (const required of providersMode ? ["key"] : ["key", "version"]) {
  if (!args[required]) throw new Error(`missing --${required}`)
}
if (args.url || args.installer) throw new Error("--url/--installer are gone: pass --windows <URL> and/or --macos <URL>")
// Linux takes one package per release; the app only tells the viewer and downloads it (a package
// manager installs it), so either format works there.
const PLATFORMS = { windows: [".exe"], macos: [".dmg"], linux: [".AppImage", ".deb"] }
if (!providersMode && !Object.keys(PLATFORMS).some((p) => typeof args[p] === "string")) {
  throw new Error("give at least one of --windows <URL>, --macos <URL>, --linux <URL>")
}

const hex = (bytes) => Buffer.from(bytes).toString("hex")
// The key file may be a bare hex key or a properties file containing one.
const keyHex = /[0-9a-f]{64}/i.exec(readFileSync(args.key, "utf8"))?.[0]
if (!keyHex) throw new Error("no 64-hex-digit private key found in the key file")
const privateKey = Buffer.from(keyHex, "hex")
if (hex(schnorr.getPublicKey(privateKey)) !== PUBKEY) throw new Error("that key is not the project update key")

let manifest, dTag, outName
if (providersMode) {
  const entries = JSON.parse(readFileSync(args.providers, "utf8"))
  manifest = {
    providers: entries.map((e) => ({
      id: e.id,
      version: e.version,
      url: e.url,
      sha256: createHash("sha256").update(readFileSync(e.file)).digest("hex"),
    })),
  }
  dTag = "anilili-desktop-providers"
  outName = "providers.json"
} else {
  const changelog = typeof args["changelog-file"] === "string"
    ? readFileSync(args["changelog-file"], "utf8").trim()
    : typeof args.changelog === "string" ? args.changelog : ""
  manifest = { version: args.version, changelog }
  for (const [platform, extensions] of Object.entries(PLATFORMS)) {
    const url = args[platform]
    if (typeof url !== "string") continue
    if (!extensions.some((e) => new URL(url).pathname.endsWith(e))) {
      throw new Error(`--${platform} must point at a ${extensions.join(" or ")} file`)
    }
    // --<platform>-file <path>: hash a local copy of the uploaded installer instead of downloading
    // it again (for when the host is slow). The URL is still asked for its size, and a copy that
    // differs from what is published by a single byte of length is refused.
    const localFile = args[`${platform}-file`]
    let installer
    if (typeof localFile === "string") {
      installer = readFileSync(localFile)
      const head = await fetch(url, { method: "HEAD" })
      if (!head.ok) throw new Error(`${url}: HTTP ${head.status}`)
      const published = Number(head.headers.get("content-length"))
      if (published !== installer.length) {
        throw new Error(`${localFile} is ${installer.length} bytes but ${url} is ${published}: not the uploaded file`)
      }
    } else {
      const response = await fetch(url)
      if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`)
      installer = Buffer.from(await response.arrayBuffer())
    }
    manifest[platform] = { url, sizeBytes: installer.length, sha256: createHash("sha256").update(installer).digest("hex") }
    console.log(`${platform}: ${installer.length} bytes, sha256 ${manifest[platform].sha256}`)
  }
  dTag = D_TAG
  outName = "desktop.json"
}
const event = {
  pubkey: PUBKEY,
  created_at: Math.floor(Date.now() / 1000),
  kind: KIND,
  tags: [["d", dTag]],
  content: JSON.stringify(manifest),
}
const id = createHash("sha256")
  .update(JSON.stringify([0, event.pubkey, event.created_at, event.kind, event.tags, event.content]))
  .digest()
const signed = { id: hex(id), ...event, sig: hex(schnorr.sign(id, privateKey)) }

const out = join(resolve(import.meta.dirname, ".."), "target", "release", outName)
writeFileSync(out, JSON.stringify(signed))
console.log(`signed -> ${out}\nupload it to the mirror as latest/${outName}`)

if (args.relays) {
  await Promise.all(
    RELAYS.map(
      (url) =>
        new Promise((done) => {
          const ws = new WebSocket(url)
          const timer = setTimeout(() => (ws.close(), console.log(`${url}: timeout`), done()), 10_000)
          ws.onopen = () => ws.send(JSON.stringify(["EVENT", signed]))
          ws.onmessage = (m) => {
            console.log(`${url}: ${m.data}`)
            clearTimeout(timer)
            ws.close()
            done()
          }
          ws.onerror = () => (clearTimeout(timer), console.log(`${url}: error`), done())
        }),
    ),
  )
}
