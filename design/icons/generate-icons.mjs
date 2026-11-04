// The Swarmy mark: a ribbon of particles that grow as they arrive, as chosen
// from design/icons (the first "Swarm S"). Writes the particle list and all
// icon files.
import { chromium } from "playwright-core"
import { writeFileSync } from "node:fs"
import { join } from "node:path"

const [, , iconsDir, outJson] = process.argv

// Exactly the chosen concept: 14 particles on a sine ribbon in a 64x64 box.
export const PARTICLES = Array.from({ length: 14 }, (_, i) => {
  const t = i / 13
  return {
    x: +(32 + 17 * Math.sin(t * Math.PI * 2)).toFixed(2),
    y: +(8 + t * 48).toFixed(2),
    r: +(1.8 + t * 3.9).toFixed(2),
    tail: t < 0.45,
  }
})
writeFileSync(outJson, JSON.stringify(PARTICLES))

const circles = (P, A) => PARTICLES.map((p) => `<circle cx="${p.x}" cy="${p.y}" r="${p.r}" fill="${p.tail ? A : P}"/>`).join("")
const BG = "#0e1510", P = "#9ccc65", A = "#5f9e3a"

// The tile from the concept card: 112px with 26px corners, a 160-degree
// gradient from #16231a to #0b120d, a 1px #223127 edge, and a 76px mark.
const TILE = 512 / 112
const icon = ({ bg = BG, rx = 26 * TILE, scale = 76 / 112, p = P, a = A, edge = true } = {}) => {
  const s = (512 * scale) / 64, off = (512 - 64 * s) / 2
  const defs = `<defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x1="168" y1="-10" x2="344" y2="522"><stop offset="0" stop-color="#16231a"/><stop offset="1" stop-color="#0b120d"/></linearGradient></defs>`
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">` +
    (bg ? `${defs}<rect width="512" height="512" rx="${rx.toFixed(1)}" fill="url(#g)"/>` +
      (edge ? `<rect x="${(TILE / 2).toFixed(2)}" y="${(TILE / 2).toFixed(2)}" width="${(512 - TILE).toFixed(2)}" height="${(512 - TILE).toFixed(2)}" rx="${(rx - TILE / 2).toFixed(1)}" fill="none" stroke="#223127" stroke-width="${TILE.toFixed(2)}"/>` : "") : "") +
    `<g transform="translate(${off.toFixed(2)} ${off.toFixed(2)}) scale(${s.toFixed(4)})">${circles(p, a)}</g></svg>`
}

writeFileSync(join(iconsDir, "icon.svg"), icon({ rx: 120, scale: 0.82 }) + "\n")

const jobs = [
  ["icon-192.png", 192, icon()],
  ["icon-512.png", 512, icon()],
  ["maskable-512.png", 512, icon({ rx: 0, scale: 0.56, edge: false })],   // inside the 80% safe zone
  ["apple-touch-icon.png", 180, icon({ rx: 0, scale: 0.66, edge: false })], // iOS rounds corners itself
  ["badge-96.png", 96, icon({ bg: null, scale: 0.92, p: "#fff", a: "#fff" })], // alpha mask
]
const b = await chromium.launch({ executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" })
for (const [name, size, svg] of jobs) {
  const page = await b.newPage({ viewport: { width: size, height: size } })
  await page.setContent(`<html><body style="margin:0;background:transparent">${svg.replace("<svg ", `<svg width="${size}" height="${size}" `)}</body></html>`)
  await page.screenshot({ path: join(iconsDir, name), omitBackground: true })
  await page.close()
}
const sheet = await b.newPage({ viewport: { width: 640, height: 200 } })
const at = (px, opts) => `<span style="display:inline-grid;place-items:center;width:${px}px;height:${px}px">${icon(opts).replace("<svg ", `<svg width="${px}" height="${px}" `)}</span>`
await sheet.setContent(`<html><body style="margin:0;padding:20px;background:#1d2b22;display:flex;gap:22px;align-items:end">
  ${at(150)}${at(96)}${at(48)}${at(32, { scale: 0.8 })}${at(20, { scale: 0.8 })}${at(16, { scale: 0.8 })}
</body></html>`)
await sheet.screenshot({ path: join(outJson, "..", "smark-sheet.png") })
await b.close()
console.log(JSON.stringify(PARTICLES))
