import type { ModelConfig } from '../types/installer';

/** Matches main app `src/web-ui/.../modelConfigs.ts` ApiFormat for presets. */
export type ApiFormat = 'openai' | 'anthropic' | 'gemini' | 'responses';

export interface ProviderUrlOption {
  url: string;
  format: ApiFormat;
  noteKey?: string;
}

export interface ProviderTemplate {
  id: string;
  nameKey: string;
  descriptionKey: string;
  baseUrl: string;
  format: ApiFormat;
  models: string[];
  helpUrl?: string;
  baseUrlOptions?: ProviderUrlOption[];
}

/** Same order as `AIModelConfig.tsx` `providerOrder`. */
export const PROVIDER_DISPLAY_ORDER: string[] = ['inner'];

export const PROVIDER_TEMPLATES: Record<string, ProviderTemplate> = {
  inner: {
    id: 'inner',
    nameKey: 'model.providers.inner.name',
    descriptionKey: 'model.providers.inner.description',
    baseUrl: 'http://7.242.99.159:8888/v1',
    format: 'openai',
    models: [],
  },
};

export function getOrderedProviders(): ProviderTemplate[] {
  const ordered: ProviderTemplate[] = [];
  for (const id of PROVIDER_DISPLAY_ORDER) {
    const template = PROVIDER_TEMPLATES[id];
    if (template) ordered.push(template);
  }
  for (const template of Object.values(PROVIDER_TEMPLATES)) {
    if (!PROVIDER_DISPLAY_ORDER.includes(template.id)) {
      ordered.push(template);
    }
  }
  return ordered;
}

export function resolveProviderFormat(template: ProviderTemplate, baseUrl: string): ApiFormat {
  if (template.baseUrlOptions && template.baseUrlOptions.length > 0) {
    const selected = template.baseUrlOptions.find((item) => item.url === baseUrl.trim());
    if (selected) return selected.format;
  }
  return template.format;
}

export function createModelConfigFromTemplate(
  template: ProviderTemplate,
  previous: ModelConfig | null
): ModelConfig {
  const modelName = previous?.modelName?.trim() || template.models[0] || '';
  const baseUrl = previous?.baseUrl?.trim() || template.baseUrl;
  return {
    provider: template.id,
    apiKey: previous?.apiKey || '',
    modelName,
    baseUrl,
    format: resolveProviderFormat(template, baseUrl),
    configName: `${template.id} - ${modelName}`.trim(),
    customRequestBody: previous?.customRequestBody,
    skipSslVerify: previous?.skipSslVerify,
    customHeaders: previous?.customHeaders,
    customHeadersMode: previous?.customHeadersMode || 'merge',
  };
}
