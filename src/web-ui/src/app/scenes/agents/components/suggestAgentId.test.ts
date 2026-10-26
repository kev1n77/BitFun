import { afterEach, expect, it, vi } from 'vitest';
import { createSuggestedAgentId, suggestAgentId } from './suggestAgentId';
afterEach(() => vi.unstubAllGlobals());
it.each([
  ['Research Helper', 'research-helper'], ['123 review', 'agent-123-review'],
  ['Résumé', 'resume'], ['中文智能体', 'agent-fallback'], ['../../', 'agent-fallback'],
])('suggests a portable ID for %s', (name, expected) => {
  expect(suggestAgentId(name, 'agent-fallback')).toBe(expected);
});

it.each([undefined, {}])('generates valid fallback IDs without randomUUID (%s)', crypto => {
  vi.stubGlobal('crypto', crypto);
  const id = createSuggestedAgentId();
  expect(id).toMatch(/^agent-[a-z0-9]+-[a-z0-9]*$/);
  expect(suggestAgentId('中文智能体', id)).toBe(id);
});
