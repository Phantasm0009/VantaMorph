const params = new URLSearchParams(self.location.search)
const scriptName = params.get("script") || "./vantamorph.js"

try {
  const appModule = await import(scriptName)
  const wasmName = scriptName.replace(".js", "_bg.wasm")

  await appModule.default(wasmName)
} catch (e) {
  console.error("worker failed to initialize:", e)
  throw e
}
