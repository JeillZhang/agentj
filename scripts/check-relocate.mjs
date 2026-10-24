#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const pkgPath = path.join(root, 'agentj-ui', 'package.json');
const appPath = path.join(root, 'agentj-ui', 'apps', 'web', 'src', 'App.tsx');
const srcDir = path.join(root, 'agentj-ui', 'apps', 'web', 'src');

function fail(msg) {
  console.error(`check-relocate FAIL: ${msg}`);
  process.exit(1);
}

if (!fs.existsSync(pkgPath)) fail(`missing ${pkgPath}`);
let pkg;
try {
  pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
} catch (e) {
  fail(`invalid JSON at ${pkgPath}: ${e.message}`);
}
if (pkg.private !== true) fail('agentj-ui/package.json private !== true');
const ws = pkg.workspaces;
if (!Array.isArray(ws)) fail('workspaces not an array');
if (!ws.includes('apps/*')) fail('workspaces missing apps/*');
if (!ws.includes('packages/*')) fail('workspaces missing packages/*');

if (!fs.existsSync(appPath)) fail(`missing ${appPath}`);
// Any web test file proves the relocated suite lives in the monorepo; pinning a
// single filename rots as soon as that file is renamed.
const webTests = fs
  .readdirSync(srcDir)
  .filter((f) => f.endsWith('.test.ts') || f.endsWith('.test.tsx'));
if (webTests.length === 0) fail(`no web tests under ${srcDir}`);

const packagesDir = path.join(root, 'agentj-ui', 'packages');
if (!fs.existsSync(packagesDir) || !fs.statSync(packagesDir).isDirectory()) {
  fail(`missing packages dir ${packagesDir}`);
}

console.log('relocate-ok');
