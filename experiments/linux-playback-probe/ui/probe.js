const invoke = (command, args) => window.__TAURI__.core.invoke(command, args)
const error = document.querySelector("#error")
async function control(action) {
  try {
    await invoke("probe_control", { action })
    error.textContent = ""
  } catch (failure) {
    error.textContent = String(failure)
  }
}
document.querySelectorAll("[data-action]").forEach((button) => {
  button.addEventListener("click", () => control(button.dataset.action))
})
document.querySelector("#panel-toggle").addEventListener("click", async () => {
  const panel = document.querySelector("#panel")
  panel.hidden = !panel.hidden
  await control(panel.hidden ? "panel-off" : "panel-on")
})
let ticks = 0
setInterval(async () => {
  document.querySelector("#heartbeat").textContent = `UI heartbeat ${++ticks}`
  try {
    document.querySelector("#status").textContent = JSON.stringify(await invoke("probe_status"))
  } catch (failure) {
    error.textContent = String(failure)
  }
}, 1000)
async function browserCheck() {
  const browser = document.querySelector("#browser")
  const features = {
    oklch: CSS.supports("color", "oklch(0.5 0.1 90)"),
    colorMix: CSS.supports("color", "color-mix(in oklab, red, blue)"),
    backdropFilter: CSS.supports("backdrop-filter", "blur(4px)"),
    hasSelector: CSS.supports("selector(:has(*))"),
    registeredProperties: typeof CSS.registerProperty === "function",
    clipboardWrite: typeof navigator.clipboard?.writeText === "function",
    clipboardRead: typeof navigator.clipboard?.readText === "function",
  }
  browser.textContent = `${navigator.userAgent} | ${JSON.stringify(features)}`
  try {
    const bytes = await new Response(new Blob(["bridge probe"]).stream().pipeThrough(new CompressionStream("gzip"))).arrayBuffer()
    browser.textContent += ` | CompressionStream gzip: ${bytes.byteLength} bytes`
  } catch (failure) {
    browser.textContent += ` | CompressionStream failed: ${failure}`
  }
}
browserCheck()
