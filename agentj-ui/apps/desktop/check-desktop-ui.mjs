import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)));
function fail(msg) {
  console.error(`desktop-ui-check: ${msg}`);
  process.exit(1);
}
function assert(cond, msg) {
  if (!cond) fail(msg);
}

const conf = JSON.parse(readFileSync(join(ROOT, "src-tauri", "tauri.conf.json"), "utf8"));
const win = conf.app?.windows?.[0];
assert(win, "tauri.conf.json has no windows[0]");
assert(win.minWidth === 900, `minWidth is ${win.minWidth}, want 900`);
assert(win.minHeight === 600, `minHeight is ${win.minHeight}, want 600`);
assert(win.width === 1100, `width is ${win.width}, want 1100`);
assert(win.height === 760, `height is ${win.height}, want 760`);

const icons = join(ROOT, "src-tauri", "icons");
const pngs = ["32x32.png", "128x128.png", "128x128@2x.png", "icon.png"];
const others = ["icon.icns", "icon.ico"];
for (const f of pngs) {
  const p = join(icons, f);
  assert(existsSync(p), `${f} missing`);
  const b = readFileSync(p);
  assert(b.length > 500, `${f} is ${b.length} bytes — placeholder?`);
  assert(b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47, `${f} is not a PNG`);
}
for (const f of others) {
  const p = join(icons, f);
  assert(existsSync(p), `${f} missing`);
  assert(readFileSync(p).length > 500, `${f} is placeholder-sized`);
}
const master = readFileSync(join(ROOT, "assets", "app-icon.png"));
assert(master.length > 2000, "assets/app-icon.png missing or too small");
assert(master.readUInt32BE(16) === 1024 && master.readUInt32BE(20) === 1024, "master is not 1024x1024");
console.log("desktop-ui-ok");
