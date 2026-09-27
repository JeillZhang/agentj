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

describe("collapsed composer contracts", () => {
  test("collapsed card keeps only the anchored action button", () => {
    const css = readCss("composer.css");
    const card = ruleBody(css, '.aj-composer-card[data-collapsed="true"]');
    expect(card).toMatch(/padding:\s*0 16px 12px/);
    expect(card).toMatch(/background-color:\s*transparent/);
    expect(card).toMatch(/box-shadow:\s*0 0 0 0 transparent/);
    expect(card).toMatch(/min-height:\s*0/);
    expect(card).toMatch(/pointer-events:\s*none/);
    const send = ruleBody(css, '.aj-composer-card[data-collapsed="true"] .send');
    expect(send).toMatch(/pointer-events:\s*auto/);
  });

  test("collapsed segments collapse and leave the tab order", () => {
    const css = readCss("composer.css");
    for (const selector of [
      '.aj-composer-card[data-collapsed="true"] .aj-composer__header',
      '.aj-composer-card[data-collapsed="true"] .aj-composer__body',
      '.aj-composer-card[data-collapsed="true"] > .ask',
    ]) {
      const body = ruleBody(css, selector);
      expect(body, selector).toMatch(/max-height:\s*0/);
      expect(body, selector).toMatch(/opacity:\s*0/);
      expect(body, selector).toMatch(/visibility:\s*hidden/);
    }
    const wrap = ruleBody(css, '.aj-composer-card[data-collapsed="true"] .comp-tools .sel-wrap');
    expect(wrap).toMatch(/max-width:\s*0/);
    expect(wrap).toMatch(/visibility:\s*hidden/);
  });

  test("collapse transitions reuse the shared motion tokens", () => {
    const css = readCss("composer.css");
    expect(ruleBody(css, ".aj-composer-card")).toMatch(
      /transition:[\s\S]*var\(--aj-motion\)[\s\S]*var\(--aj-ease\)/,
    );
    expect(ruleBody(css, ".aj-composer__header")).toMatch(
      /transition:[\s\S]*var\(--aj-motion\)[\s\S]*var\(--aj-ease\)/,
    );
    expect(ruleBody(css, ".aj-composer-card > .ask")).toMatch(
      /transition:[\s\S]*var\(--aj-motion\)/,
    );
    const reducedStart = css.indexOf("@media (prefers-reduced-motion: reduce)");
    expect(reducedStart).toBeGreaterThanOrEqual(0);
    const reducedBlock = css.slice(reducedStart);
    expect(reducedBlock).toMatch(/\.aj-composer-card[^{]*\{[^}]*transition:\s*none\s*!important/);
  });

  test("collapsed rules never clip popovers (no overflow hidden on the card)", () => {
    const css = readCss("composer.css");
    const card = ruleBody(css, '.aj-composer-card[data-collapsed="true"]');
    expect(card).not.toMatch(/overflow/);
  });

  test("BUG-016 — send button right-anchors itself when the toolbar wraps (zero X drift)", () => {
    // At 1240–1280px the center column is too narrow for all four selectors +
    // spacer + send, so .comp-tools (flex-wrap: wrap) pushes the send button
    // onto its own second line. With default left alignment it then sits at the
    // line's left edge while the collapsed single-line state (selectors
    // max-width:0, spacer flex:1) pins it to the right edge → dx≈370–400px.
    // margin-left:auto on the button keeps it at the same right edge in both
    // states, preserving the "X/Y zero displacement (±1px)" acceptance promise.
    const send = ruleBody(readCss("composer.css"), ".aj-button--send");
    expect(send).toMatch(/margin-left:\s*auto/);
    // Right-anchoring must stay on the button alone: a global justify-content
    // change on .comp-tools would right-align every wrapped chip row on narrow
    // viewports (390px fallback), so selectors must keep their left alignment.
    const tools = ruleBody(readCss("options.css"), ".comp-tools");
    expect(tools).not.toMatch(/justify-content:\s*flex-end/);
  });
});
