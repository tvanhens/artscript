import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

// These packages are CommonJS. The ESM namespace only exposes `default`.
import oniguruma from 'vscode-oniguruma';
import vsctm from 'vscode-textmate';

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const extension = path.resolve(here, '..');
const workspace = path.resolve(extension, '../..');

const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
await oniguruma.loadWASM(wasm);

const scopes = JSON.parse(fs.readFileSync(path.join(here, 'scopes.json'), 'utf8'));
const manifest = JSON.parse(
  fs.readFileSync(path.join(workspace, 'target/art-ribbons/manifest.json'), 'utf8'),
);

const registry = new vsctm.Registry({
  onigLib: Promise.resolve({
    createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
    createOnigString: (value) => new oniguruma.OnigString(value),
  }),
  loadGrammar: async (scopeName) => {
    if (scopeName !== 'source.art') {
      return null;
    }
    const file = path.join(extension, 'syntaxes/artscript.tmLanguage.json');
    return vsctm.parseRawGrammar(fs.readFileSync(file, 'utf8'), 'artscript.tmLanguage.json');
  },
});

const grammar = await registry.loadGrammar('source.art');
assert.ok(grammar, 'source.art grammar did not load');

for (const entry of manifest) {
  test(entry.source, () => {
    const src = fs.readFileSync(path.join(workspace, entry.source), 'utf8');
    const expected = fs.readFileSync(path.join(workspace, 'target/art-ribbons', entry.ribbon), 'utf8');
    const sourceLines = src.split(/\r?\n/);
    const lexerLines = expected.split('\n');
    let ruleStack = vsctm.INITIAL;
    const grammarLines = [];
    for (const line of sourceLines) {
      const tokenized = grammar.tokenizeLine(line, ruleStack);
      ruleStack = tokenized.ruleStack;
      let row = '';
      for (const token of tokenized.tokens) {
        // The tokenizer appends a newline before matching, and the last token can include it.
        const end = Math.min(token.endIndex, line.length);
        if (end <= token.startIndex) {
          continue;
        }
        const text = line.slice(token.startIndex, end);
        row += letterFor(token.scopes, text).repeat(end - token.startIndex);
      }
      grammarLines.push(row);
    }
    const count = Math.max(sourceLines.length, lexerLines.length, grammarLines.length);
    for (let i = 0; i < count; i++) {
      const lexer = lexerLines[i] ?? '';
      const actual = grammarLines[i] ?? '';
      if (lexer !== actual) {
        assert.fail(
          `${entry.source}:${i + 1}\n  source    ${sourceLines[i] ?? ''}\n  lexer     ${lexer}\n  grammar   ${actual}`,
        );
      }
    }
  });
}

function letterFor(tokenScopes, text) {
  for (const scope of tokenScopes) {
    if (scope.endsWith('.art') && Object.hasOwn(scopes, scope)) {
      return scopes[scope];
    }
  }
  if ([...text].every((ch) => ch === ' ' || ch === '\t')) {
    return ' ';
  }
  return 'i';
}
