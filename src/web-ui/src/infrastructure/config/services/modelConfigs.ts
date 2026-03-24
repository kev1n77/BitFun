import { ModelConfig, ProviderTemplate, ApiFormat } from '../../../shared/types';
import { configManager } from './ConfigManager';
import { i18nService } from '@/infrastructure/i18n';
import { createLogger } from '@/shared/utils/logger';
import { extractProviderSegmentFromBaseUrl, matchProviderCatalogItemByBaseUrl } from './providerCatalog';

const log = createLogger('ModelConfigManager');
const t = (key: string, options?: Record<string, unknown>) => i18nService.t(key, options);

type ProviderConfigLike = {
  name?: string;
  model_name?: string;
  base_url?: string;
};

function inferProviderTemplate(config: ProviderConfigLike): ProviderTemplate | undefined {
  const matchedCatalogItem = matchProviderCatalogItemByBaseUrl(config.base_url);
  // Safe module-level forward reference: PROVIDER_TEMPLATES is initialized before this runs.
  // eslint-disable-next-line @typescript-eslint/no-use-before-define
  return matchedCatalogItem ? PROVIDER_TEMPLATES[matchedCatalogItem.id] : undefined;
}

export function getProviderTemplateId(config: ProviderConfigLike): string | undefined {
  return inferProviderTemplate(config)?.id;
}

export function getProviderDisplayName(config: ProviderConfigLike): string {
  const inferredTemplate = inferProviderTemplate(config);
  if (inferredTemplate) {
    return t(`settings/ai-model:providers.${inferredTemplate.id}.name`);
  }

  const rawName = config.name?.trim() || '';
  const rawModelName = config.model_name?.trim() || '';
  if (rawName && rawModelName) {
    const dashedSuffix = ` - ${rawModelName}`;
    const slashSuffix = `/${rawModelName}`;

    if (rawName.endsWith(dashedSuffix)) {
      return rawName.slice(0, -dashedSuffix.length).trim();
    }
    if (rawName.endsWith(slashSuffix)) {
      return rawName.slice(0, -slashSuffix.length).trim();
    }
  }

  return rawName || extractProviderSegmentFromBaseUrl(config.base_url) || t('settings/ai-model:providerSelection.customTitle');
}

export function getModelDisplayName(config: ProviderConfigLike): string {
  const providerName = getProviderDisplayName(config);
  const modelName = config.model_name?.trim() || '';

  if (!providerName) return modelName;
  if (!modelName) return providerName;

  return `${providerName}/${modelName}`;
}

export const PROVIDER_TEMPLATES: Record<string, ProviderTemplate> = {
  inner: {
    id: 'inner',
    name: 'Central Software Institute',
    baseUrl: 'http://7.242.99.159:8888/v1',
    format: 'openai',
    models: [],
    requiresApiKey: false,
    description: 'Inner AI Provider'
  }
};

type ConfigChangeListener = (configs: ModelConfig[]) => void;

class ModelConfigManager {
  private configs: ModelConfig[] = [];
  private listeners: Set<ConfigChangeListener> = new Set();

  constructor() {
    this.loadConfigs();
  }

  // Listener management
  addListener(listener: ConfigChangeListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  // Notify listeners
  private notifyListeners(): void {
    const configsCopy = [...this.configs];
    
    this.listeners.forEach(listener => {
      try {
        listener(configsCopy);
      } catch (error) {
        log.error('Error in config change listener', error);
      }
    });
  }

  // New architecture: load via the unified config manager.
  private loadConfigs(): void {
    // Start with an empty set, then sync async.
    this.configs = [];
    
    // Async load the real config.
    this.syncFromConfigManager().catch(error => {
      log.error('Failed to load configs', error);
      this.configs = [];
      this.notifyListeners();
    });
  }

  // New architecture: sync from the unified config manager.
  private async syncFromConfigManager(): Promise<void> {
    try {
      // Fetch AI model configuration from the unified config manager.
      const aiModels = await configManager.getConfig<any[]>('ai.models');
      
      if (aiModels && aiModels.length > 0) {
        // Convert backend shape -> frontend shape.
        this.configs = aiModels.map(model => ({
          id: model.id,
          name: model.name,
          baseUrl: model.base_url,
          apiKey: model.api_key,
          modelName: model.model_name,
          format: model.provider as ApiFormat,
          description: model.description || t('settings/ai-model:messages.defaultDescription', { name: model.name }),
          isBuiltIn: false,
          contextWindow: model.context_window,
          maxTokens: model.max_tokens
        }));
      } else {
        // No config available from backend.
        this.configs = [];
      }
      
      this.notifyListeners();
    } catch (error) {
      log.error('Failed to load configs from backend', error);
      this.configs = [];
      this.notifyListeners();
    }
  }

  // New architecture: persist via the unified config manager.
  private async saveConfigs(): Promise<void> {
    try {
      // Convert to backend shape.
      const backendConfigs = this.configs.map(config => ({
        id: config.id,
        name: config.name,
        model_name: config.modelName,
        provider: config.format,
        base_url: config.baseUrl,
        api_key: config.apiKey || '',
        enabled: true,
        description: config.description,
        context_window: config.contextWindow,
        max_tokens: config.maxTokens
      }));
      
      // Save to the unified config system.
      await configManager.setConfig('ai.models', backendConfigs);
      
      this.notifyListeners();
    } catch (error) {
      log.error('Failed to save configs', error);
      throw error;
    }
  }

  // Reload configuration (public).
  async reload(): Promise<void> {
    await this.syncFromConfigManager();
  }

  // Read operations
  getAllConfigs(): ModelConfig[] {
    return [...this.configs];
  }

  getConfigById(id: string): ModelConfig | undefined {
    return this.configs.find(config => config.id === id);
  }

  // Write operations
  addConfig(config: Omit<ModelConfig, 'id'>): ModelConfig {
    const newConfig: ModelConfig = {
      ...config,
      id: this.generateId(),
    };
    this.configs.push(newConfig);
    
    // Persist async.
    this.saveConfigs().catch(error => {
      log.error('Failed to save new config', error);
    });
    
    return newConfig;
  }

  updateConfig(id: string, updates: Partial<ModelConfig>): boolean {
    const index = this.configs.findIndex(config => config.id === id);
    if (index === -1) return false;

    this.configs[index] = { ...this.configs[index], ...updates };
    
    // Persist async.
    this.saveConfigs().catch(error => {
      log.error('Failed to update config', { configId: id, error });
    });
    
    return true;
  }

  deleteConfig(id: string): boolean {
    const index = this.configs.findIndex(config => config.id === id);
    if (index === -1) return false;

    this.configs.splice(index, 1);
    
    this.saveConfigs().catch(error => {
      log.error('Failed to delete config', { configId: id, error });
    });
    
    return true;
  }

  cloneConfig(id: string): ModelConfig | null {
    const config = this.getConfigById(id);
    if (!config) return null;

    const cloned = this.addConfig({
      ...config,
      name: t('settings/ai-model:messages.cloneName', { name: config.name }),
      isBuiltIn: false
    });
    return cloned;
  }

  private generateId(): string {
    return `config_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  createFromTemplate(providerId: string, modelName: string): ModelConfig | null {
    const template = PROVIDER_TEMPLATES[providerId];
    if (!template) return null;

    return this.addConfig({
      name: template.name,
      baseUrl: template.baseUrl,
      modelName,
      format: template.format,
      description: t('settings/ai-model:messages.templateDescription', { description: template.description, modelName }),
      isBuiltIn: false
    });
  }

  resetToDefault(): void {
    this.configs = [];
    this.saveConfigs().catch(error => {
      log.error('Failed to reset configs', error);
    });
  }
}

export const getAllTemplates = (): ProviderTemplate[] => {
  return Object.values(PROVIDER_TEMPLATES);
};

export const getFormatDisplayName = (format: ApiFormat): string => {
  switch (format) {
    case 'openai':
      return t('settings/ai-model:formats.openaiCompatible');
    case 'responses':
      return t('settings/ai-model:formats.responsesApi');
    case 'anthropic':
      return t('settings/ai-model:formats.claudeApi');
    case 'gemini':
      return t('settings/ai-model:formats.geminiApi');
    default:
      return format;
  }
};

export const modelConfigManager = new ModelConfigManager();

if (typeof window !== 'undefined' && process.env.NODE_ENV === 'development') {
  (window as any).modelConfigManager = modelConfigManager;
}
