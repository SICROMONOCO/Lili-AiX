// Read-only source inventory for the Linux feasibility study. Does not compile or run Linux code.
// Usage: node scripts/linux-port-audit.mjs
import { createHash } from "node:crypto"
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs"
import { dirname, extname, join, relative, resolve } from "node:path"
import ts from "typescript"

const root = resolve(import.meta.dirname, "..")
const included = new Set([".rs", ".toml", ".ts", ".tsx", ".css", ".html", ".mjs", ".sh", ".ps1", ".wit", ".json", ".patch", ".svg"])
const excluded = new Set(["target", "node_modules", "vendor", "gen", ".git", "dist"])
const paths = []
function collect(path) {
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    if (excluded.has(entry.name) || entry.isSymbolicLink()) continue
    const child = join(path, entry.name)
    if (entry.isDirectory()) collect(child)
    else if (included.has(extname(child))) paths.push(child)
  }
}
for (const directory of ["src", "src-tauri", "crates", "providers", "scripts", "wit", "fixtures", "patches", ".cargo", "experiments/linux-playback-probe"]) {
  collect(join(root, directory))
}
for (const file of ["Cargo.toml", "rust-toolchain.toml", "package.json", "vite.config.ts", "tsconfig.json", "tsconfig.app.json", "tsconfig.node.json"]) {
  if (existsSync(join(root, file))) paths.push(join(root, file))
}
const sources = [...new Set(paths)].sort().map((path) => {
  const text = readFileSync(path, "utf8")
  return { path: relative(root, path).replaceAll("\\", "/"), text }
})
const byPath = new Map(sources.map((source) => [source.path, source]))
const byLower = new Map()
for (const source of sources) {
  const lower = source.path.toLowerCase()
  byLower.set(lower, [...(byLower.get(lower) ?? []), source.path])
}

const references = []
const declarations = []
const unresolvedImports = []
for (const source of sources) {
  const lines = source.text.split(/\r?\n/)
  lines.forEach((text, index) => {
    const categories = []
    if (/cfg!?\(|target_os|target_family|process\.platform|process\.arch|navigator\.platform/.test(text) && !/^\s*#\[cfg\(test\)\]/.test(text)) categories.push("platform")
    if (/Command::new|execFileSync|execSync|powershell|taskkill|cygpath/.test(text)) categories.push("process")
    if (/clipboard|outerPosition|setPosition|setAlwaysOnTop|setDecorations/.test(text)) categories.push("desktop-ui")
    if (categories.length) references.push({ file: source.path, line: index + 1, categories, text: text.trim() })
  })
  if (source.path.startsWith("src-tauri/src/")) {
    const expression = /#\[tauri::command(?:\(([^\]]*)\))?\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?(async\s+)?fn\s+(\w+)/g
    for (const match of source.text.matchAll(expression)) {
      const module = source.path.split("/").at(-1).replace(".rs", "")
      const command = module === "main" ? match[3] : `${module}::${match[3]}`
      declarations.push({ command, file: source.path, line: source.text.slice(0, match.index).split("\n").length, execution: match[2] ? "async" : match[1]?.includes("async") ? "worker" : "main-thread" })
    }
  }
  if (source.path.startsWith("src/") && /\.tsx?$/.test(source.path)) {
    const parsed = ts.createSourceFile(source.path, source.text, ts.ScriptTarget.Latest, true)
    function visit(node) {
      let specifier
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) specifier = node.moduleSpecifier.text
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) specifier = node.arguments[0].text
      if (specifier?.startsWith("@/") || specifier?.startsWith(".")) {
        const clean = specifier.split("?")[0]
        const absolute = clean.startsWith("@/") ? join(root, "src", clean.slice(2)) : resolve(root, dirname(source.path), clean)
        const base = relative(root, absolute).replaceAll("\\", "/")
        const candidates = [base, ...[".ts", ".tsx", ".js", ".jsx", ".json"].map((extension) => base + extension), base + "/index.ts", base + "/index.tsx"]
        if (!candidates.some((candidate) => byPath.has(candidate))) {
          const caseMatches = candidates.flatMap((candidate) => byLower.get(candidate.toLowerCase()) ?? [])
          unresolvedImports.push({ file: source.path, specifier, caseMatches })
        }
      }
      ts.forEachChild(node, visit)
    }
    visit(parsed)
  }
}
const main = byPath.get("src-tauri/src/main.rs").text
const handler = /generate_handler!\[([\s\S]*?)\]/.exec(main)?.[1] ?? ""
const registered = handler.split(",").map((name) => name.trim()).filter(Boolean)
const declared = new Set(declarations.map((declaration) => declaration.command))
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"))
const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"))
const tauriNpmSpecs = Object.entries({ ...packageJson.dependencies, ...packageJson.devDependencies })
  .filter(([name]) => name.startsWith("@tauri-apps/"))
  .map(([name, specifier]) => ({ name, specifier, exact: /^\d+\.\d+\.\d+(?:-[\w.]+)?$/.test(specifier), locked: lock.packages[`node_modules/${name}`]?.version }))
const bindings = ["@rolldown/binding-linux-x64-gnu", "@tailwindcss/oxide-linux-x64-gnu", "@tauri-apps/cli-linux-x64-gnu"]
const groupCounts = {}
for (const source of sources) {
  const group = source.path.split("/")[0]
  groupCounts[group] = (groupCounts[group] ?? 0) + 1
}
const report = {
  scope: "Static source inventory on Windows; no claim of Linux compilation or runtime validation.",
  files: sources.map(({ path, text }) => ({ path, lines: text.split(/\r?\n/).length, sha256: createHash("sha256").update(text).digest("hex") })),
  groupCounts,
  references,
  commands: { declarations, registered, missingDeclarations: registered.filter((name) => !declared.has(name)) },
  imports: { unresolved: unresolvedImports, caseCollisions: [...byLower.values()].filter((matches) => matches.length > 1) },
  tauriNpmSpecs,
  linuxNpmBindings: bindings.map((name) => ({ name, present: !!lock.packages[`node_modules/${name}`] })),
}
const output = join(root, "target", "linux-audit", "source-audit.json")
mkdirSync(dirname(output), { recursive: true })
writeFileSync(output, JSON.stringify(report, null, 2) + "\n")
console.log(JSON.stringify({
  output: relative(root, output).replaceAll("\\", "/"),
  files: report.files.length,
  groupCounts,
  references: references.length,
  declaredCommands: declarations.length,
  registeredCommands: registered.length,
  missingDeclarations: report.commands.missingDeclarations,
  unresolvedImports,
  caseCollisions: report.imports.caseCollisions,
  tauriNpmSpecs,
  linuxNpmBindings: report.linuxNpmBindings,
}, null, 2))
