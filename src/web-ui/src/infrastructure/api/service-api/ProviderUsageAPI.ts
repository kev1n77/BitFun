/**
 * Provider Usage Statistics API
 *
 * Fetches API usage statistics from internal provider dashboard (http://7.242.99.159:8888)
 */

import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '@/shared/utils/logger';

const log = createLogger('ProviderUsageAPI');

/**
 * Check if a base URL matches the internal provider
 */
export function isInternalProvider(baseUrl: string): boolean {
  return baseUrl.includes('7.242.99.159') || baseUrl.includes('internal');
}

/**
 * Usage statistics response from /api/user/key-info
 * Note: API returns snake_case JSON, using snake_case directly
 */
export interface UsageStats {
  spend: number;
  total_input_tokens: number;
  total_output_tokens: number;
  created_at?: string;
  key_alias?: string;
  key_name?: string;
  rpm_limit: number;
  tpm_limit?: number;
  max_budget?: number;
  budget_duration?: string;
  expires?: string;
  blocked: boolean;
  token_prefix?: string;
}

/**
 * Plan info response from /api/user/usage (rate limit data)
 */
export interface PlanInfo {
  concurrency?: number;
  concurrency_limit: number;
  plan_name: string;
  windows: UsageWindow[];
}

export interface UsageWindow {
  cache_key: string;
  count: number;
  elapsed_secs: number;
  limit: number;
  window_secs: number;
}

/**
 * Schedule item for rate limits
 */
export interface ScheduleItem {
  hours: string;
  rpm_limit: number;
  concurrency_limit: number;
  window_limits: number[][];
}

/**
 * Key info response from /api/user/plan (rpm schedule)
 */
export interface KeyInfo {
  concurrency_limit: number;
  plan_name: string;
  rpm_limit: number;
  schedule: ScheduleItem[];
  window_limits: number[][];
}

/**
 * Single usage log entry
 */
export interface UsageLogEntry {
  created_at: string;
  model: string;
  api_path: string;
  status_code: number;
  error_type?: string;
  error_message?: string;
  input_tokens?: number;
  output_tokens?: number;
  duration_ms?: number;
  is_stream: boolean;
}

/**
 * Usage logs response
 */
export interface UsageLogs {
  logs: UsageLogEntry[];
  page: number;
  per_page: number;
  total: number;
}

/**
 * Combined usage statistics
 */
export interface CombinedUsageStats {
  usage: UsageStats;
  plan: PlanInfo;
  key_info: KeyInfo;
}

/**
 * Request for getting usage stats
 */
interface GetProviderUsageRequest {
  apiKey: string;
}

/**
 * Request for getting usage logs
 */
interface GetProviderLogsRequest {
  apiKey: string;
  page?: number;
  perPage?: number;
}

/**
 * Provider Usage Statistics API
 */
export const providerUsageAPI = {
  /**
   * Get combined usage statistics for a provider
   */
  async getUsageStats(apiKey: string): Promise<CombinedUsageStats> {
    log.info('Fetching provider usage stats');
    try {
      const result = await invoke<CombinedUsageStats>('get_provider_usage_stats', {
        request: { apiKey } as GetProviderUsageRequest,
      });
      log.info('Successfully fetched usage stats');
      return result;
    } catch (error) {
      log.error('Failed to fetch usage stats:', error);
      throw error;
    }
  },

  /**
   * Get usage logs for a provider
   */
  async getUsageLogs(
    apiKey: string,
    page: number = 1,
    perPage: number = 50
  ): Promise<UsageLogs> {
    log.info(`Fetching provider usage logs: page=${page}, perPage=${perPage}`);
    try {
      const result = await invoke<UsageLogs>('get_provider_usage_logs', {
        request: {
          apiKey,
          page,
          perPage,
        } as GetProviderLogsRequest,
      });
      log.info(`Successfully fetched ${result.logs.length} usage logs`);
      return result;
    } catch (error) {
      log.error('Failed to fetch usage logs:', error);
      throw error;
    }
  },
};

/**
 * Format token count for display
 */
export function formatTokenCount(tokens: number): string {
  if (tokens >= 1_000_000) {
    return `${(tokens / 1_000_000).toFixed(1)}M`;
  }
  if (tokens >= 1_000) {
    return `${(tokens / 1_000).toFixed(1)}K`;
  }
  return tokens.toString();
}

/**
 * Format currency for display
 */
export function formatCurrency(amount: number): string {
  return `$${amount.toFixed(2)}`;
}

/**
 * Format date for display
 */
export function formatDate(isoString: string): string {
  try {
    const date = new Date(isoString);
    return date.toLocaleString();
  } catch {
    return isoString;
  }
}

/**
 * Get status display text
 */
export function getStatusDisplay(statusCode: number): string {
  switch (statusCode) {
    case 200:
      return 'Success';
    case 400:
      return 'Bad Request';
    case 401:
      return 'Unauthorized';
    case 403:
      return 'Forbidden';
    case 404:
      return 'Not Found';
    case 429:
      return 'Rate Limited';
    case 500:
      return 'Server Error';
    default:
      return `HTTP ${statusCode}`;
  }
}