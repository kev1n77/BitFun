/** Generate a portable ID even in WebViews without randomUUID. */
export function createSuggestedAgentId(): string {
  const suffix = typeof globalThis.crypto?.randomUUID === 'function'
    ? globalThis.crypto.randomUUID().slice(0, 8)
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  return `agent-${suffix}`;
}

/** IDs are portable filenames; display names remain unrestricted and localized. */
export function suggestAgentId(name: string, fallback: string): string {
  const slug = name.normalize('NFKD').replace(/[\u0300-\u036f]/g, '')
    .toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
  return slug ? (/^[a-z]/.test(slug) ? slug : `agent-${slug}`) : fallback;
}
