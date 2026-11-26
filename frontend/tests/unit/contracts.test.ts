// @vitest-environment node
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const src = fileURLToPath(new URL('../../src/', import.meta.url)).replaceAll('\\', '/');

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(`${dir}${e.name}/`) : [`${dir}${e.name}`],
  );
}

describe('generated API contracts', () => {
  it('re-exports every generated type from src/types/api.ts', () => {
    const generated = readdirSync(`${src}types/generated`)
      .filter((f) => f.endsWith('.ts'))
      .map((f) => f.replace(/\.ts$/, ''));
    const barrel = readFileSync(`${src}types/api.ts`, 'utf8');
    expect(generated.length).toBeGreaterThan(0);
    for (const name of generated) {
      expect(barrel).toContain(`export type { ${name} } from './generated/${name}.ts';`);
    }
  });
});

describe('fixture isolation', () => {
  it('only the design-system showcase imports fixtures', () => {
    const offenders = walk(src)
      .filter((f) => /\.(ts|tsx)$/.test(f) && !f.includes('/fixtures/'))
      .filter((f) => /from ['"](@\/|\.\.?\/)(.*\/)?fixtures\//.test(readFileSync(f, 'utf8')))
      .map((f) => f.slice(src.length))
      .filter((f) => f !== 'pages/DesignSystemPage.tsx');
    expect(offenders).toEqual([]);
  });
});
