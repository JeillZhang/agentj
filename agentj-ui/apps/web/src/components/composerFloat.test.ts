import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";

// The floating composer is a layout/scroll change jsdom cannot measure, so —
// like bug003.test.tsx — these lock the CSS contracts that keep it centered.

function readCss(name: string): string {
  return readFileSync(join(import.meta.dirname, name), "utf8");
}

function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escaped}\\s*\\{[^}]*\\}`));
  expect(match, `${selector} rule missing`).not.toBeNull();
  return match![0];
}

describe("floating composer layout", () => {
  test("composer.css floats .aj-composer-card at 75% width, centered", () => {
    const body = ruleBody(readCss("composer.css"), ".aj-composer-card");
    expect(body).toMatch(/position:\s*absolute/);
    expect(body).not.toMatch(/position:\s*sticky/);
    expect(body).toMatch(/bottom:\s*16px/);
    expect(body).toMatch(/left:\s*0/);
    expect(body).toMatch(/right:\s*0/);
    expect(body).toMatch(/width:\s*75%/);
    expect(body).toMatch(/min-width:\s*min\(340px,\s*calc\(100% - 16px\)\)/);
    expect(body).toMatch(/margin:\s*0 auto/);
    expect(body).toMatch(/max-height:\s*calc\(100%\s*-\s*32px\)/);
    expect(body).toMatch(/min-height:\s*120px/);
    expect(body).toMatch(/z-index:\s*2/);
  });

  test("layout.css splits the center-column rule (conv 100%, composer 75% + auto)", () => {
    const css = readCss("layout.css");
    const conv = ruleBody(css, ".aj-shell__center .aj-conv");
    expect(conv).toMatch(/width:\s*100%/);
    expect(conv).toMatch(/max-width:\s*none/);
    expect(conv).toMatch(/margin:\s*0;/);
    const composer = ruleBody(css, ".aj-shell__center .aj-composer-card");
    expect(composer).toMatch(/width:\s*75%/);
    expect(composer).toMatch(/max-width:\s*none/);
    expect(composer).toMatch(/margin:\s*0 auto/);
    expect(composer).not.toMatch(/width:\s*100%/);
  });

  test("conversation.css reserves scroll clearance for the floating composer", () => {
    const body = ruleBody(readCss("conversation.css"), ".aj-conv");
    expect(body).toMatch(
      /padding:\s*14px 14px calc\(var\(--aj-composer-h,\s*152px\)\s*\+\s*32px\)/,
    );
    expect(body).toMatch(
      /scroll-padding-bottom:\s*calc\(var\(--aj-composer-h,\s*152px\)\s*\+\s*32px\)/,
    );
  });

  test("options.css lets the toolbar wrap on narrow viewports", () => {
    const body = ruleBody(readCss("options.css"), ".comp-tools");
    expect(body).toMatch(/flex-wrap:\s*wrap/);
  });

  test("composer.css right-aligns the header-meta group beside the model chip", () => {
    const css = readCss("composer.css");
    const header = ruleBody(css, ".aj-composer__header");
    expect(header).toMatch(/flex-wrap:\s*wrap/);
    expect(header).toMatch(/justify-content:\s*space-between/);
    const meta = ruleBody(css, ".aj-composer__header-meta");
    expect(meta).toMatch(/margin-left:\s*auto/);
    expect(meta).toMatch(/gap:\s*8px/);
    expect(meta).toMatch(/flex-wrap:\s*wrap/);
    expect(meta).toMatch(/justify-content:\s*flex-end/);
    const usage = ruleBody(css, ".aj-composer__usage");
    expect(usage).toMatch(/white-space:\s*nowrap/);
    expect(usage).toMatch(/font-size:\s*12px/);
    expect(usage).toMatch(/font-variant-numeric:\s*tabular-nums/);
  });

  test("options.css no longer defines the retired .comp-usage rule", () => {
    expect(readCss("options.css")).not.toContain(".comp-usage");
  });

  test("App.css keeps the .aj-card containing block + clipping dependency", () => {
    const app = readFileSync(join(import.meta.dirname, "..", "App.css"), "utf8");
    const card = ruleBody(app, ".aj-card");
    expect(card).toMatch(/position:\s*relative/);
    expect(card).toMatch(/overflow:\s*hidden/);
  });
});
