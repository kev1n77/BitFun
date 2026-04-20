/**
 * Usage Statistics Modal Component
 *
 * Displays API usage statistics for internal provider models.
 */

import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Modal,
  Button,
  CubeLoading,
  Tooltip,
  Card,
  Badge,
} from '@/component-library';
import {
  providerUsageAPI,
  formatTokenCount,
  formatDate,
  getStatusDisplay,
  type CombinedUsageStats,
  type UsageLogs,
  type UsageLogEntry,
  type UsageWindow,
} from '@/infrastructure/api';
import {
  RefreshCw,
  TrendingUp,
  TrendingDown,
  Activity,
  Clock,
  AlertCircle,
  CheckCircle,
  XCircle,
  ChevronLeft,
  ChevronRight,
  BarChart2,
  FileText,
} from 'lucide-react';
import './UsageStatsModal.scss';

interface UsageStatsModalProps {
  isOpen: boolean;
  onClose: () => void;
  apiKey: string;
  modelName: string;
  baseUrl: string;
}

type TabType = 'overview' | 'logs';

export const UsageStatsModal: React.FC<UsageStatsModalProps> = ({
  isOpen,
  onClose,
  apiKey,
  modelName,
  baseUrl,
}) => {
  const { t } = useTranslation('settings/ai-model');
  const [activeTab, setActiveTab] = useState<TabType>('overview');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<CombinedUsageStats | null>(null);
  const [logs, setLogs] = useState<UsageLogs | null>(null);
  const [logsPage, setLogsPage] = useState(1);
  const [logsLoading, setLogsLoading] = useState(false);

  const fetchStats = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await providerUsageAPI.getUsageStats(apiKey);
      setStats(result);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      console.error('Failed to fetch usage stats:', err);
    } finally {
      setLoading(false);
    }
  }, [apiKey]);

  const fetchLogs = useCallback(async (page: number) => {
    setLogsLoading(true);
    try {
      const result = await providerUsageAPI.getUsageLogs(apiKey, page, 20);
      setLogs(result);
      setLogsPage(page);
    } catch (err) {
      console.error('Failed to fetch usage logs:', err);
    } finally {
      setLogsLoading(false);
    }
  }, [apiKey]);

  useEffect(() => {
    if (isOpen) {
      fetchStats();
      fetchLogs(1);
    }
  }, [isOpen, fetchStats, fetchLogs]);

  const handleRefresh = () => {
    fetchStats();
    fetchLogs(1);
  };

  const totalTokens = stats
    ? stats.usage.total_input_tokens + stats.usage.total_output_tokens
    : 0;

  const renderStatusBadge = (statusCode: number) => {
    if (statusCode === 200) {
      return (
        <Badge variant="success">
          <CheckCircle size={12} />
          {getStatusDisplay(statusCode)}
        </Badge>
      );
    }
    if (statusCode >= 400 && statusCode < 500) {
      return (
        <Badge variant="warning">
          <AlertCircle size={12} />
          {getStatusDisplay(statusCode)}
        </Badge>
      );
    }
    if (statusCode >= 500) {
      return (
        <Badge variant="error">
          <XCircle size={12} />
          {getStatusDisplay(statusCode)}
        </Badge>
      );
    }
    return (
      <Badge variant="neutral">
        {getStatusDisplay(statusCode)}
      </Badge>
    );
  };

  const renderLogEntry = (log: UsageLogEntry) => (
    <div key={log.created_at} className="usage-stats-modal__log-entry">
      <div className="usage-stats-modal__log-header">
        <span className="usage-stats-modal__log-model">{log.model}</span>
        {renderStatusBadge(log.status_code)}
        <span className="usage-stats-modal__log-time">
          <Clock size={12} />
          {formatDate(log.created_at)}
        </span>
      </div>
      {log.error_message && (
        <div className="usage-stats-modal__log-error">
          <AlertCircle size={12} />
          {log.error_message}
        </div>
      )}
      {(log.input_tokens || log.output_tokens) && (
        <div className="usage-stats-modal__log-tokens">
          {log.input_tokens && (
            <span>
              <TrendingUp size={12} />
              {formatTokenCount(log.input_tokens)} in
            </span>
          )}
          {log.output_tokens && (
            <span>
              <TrendingDown size={12} />
              {formatTokenCount(log.output_tokens)} out
            </span>
          )}
          {log.duration_ms && (
            <span className="usage-stats-modal__log-duration">
              {log.duration_ms}ms
            </span>
          )}
        </div>
      )}
    </div>
  );

  const renderOverview = () => {
    if (loading) {
      return (
        <div className="usage-stats-modal__loading">
          <CubeLoading size="large" />
          <span>{t('usageStats.loading')}</span>
        </div>
      );
    }

    if (error) {
      return (
        <div className="usage-stats-modal__error">
          <AlertCircle size={24} />
          <span>{error}</span>
          <Button variant="secondary" onClick={handleRefresh}>
            {t('usageStats.retry')}
          </Button>
        </div>
      );
    }

    if (!stats) return null;

    return (
      <div className="usage-stats-modal__overview">
        {/* 5-Hour Window Card */}
        <Card className="usage-stats-modal__stat-card">
          <div className="usage-stats-modal__stat-header">
            <Activity size={16} />
            <span>{t('usageStats.window5h')}</span>
          </div>
          {(() => {
            // Find the 5-hour window (18000 seconds = 5 hours)
            const window5h = stats.plan.windows.find((w: UsageWindow) => w.window_secs === 18000);
            if (window5h) {
              return (
                <div className="usage-stats-modal__stat-value">
                  {window5h.count} / {window5h.limit}
                </div>
              );
            }
            return (
              <div className="usage-stats-modal__stat-value">
                {t('usageStats.noData')}
              </div>
            );
          })()}
        </Card>

        {/* Tokens Card */}
        <Card className="usage-stats-modal__stat-card">
          <div className="usage-stats-modal__stat-header">
            <Activity size={16} />
            <span>{t('usageStats.tokenUsage')}</span>
          </div>
          <div className="usage-stats-modal__stat-value">
            {formatTokenCount(totalTokens)}
          </div>
          <div className="usage-stats-modal__stat-breakdown">
            <div className="usage-stats-modal__stat-breakdown-item">
              <span className="usage-stats-modal__stat-breakdown-label">
                {t('usageStats.inputTokens')}
              </span>
              <span className="usage-stats-modal__stat-breakdown-value">
                {formatTokenCount(stats.usage.total_input_tokens)}
              </span>
            </div>
            <div className="usage-stats-modal__stat-breakdown-item">
              <span className="usage-stats-modal__stat-breakdown-label">
                {t('usageStats.outputTokens')}
              </span>
              <span className="usage-stats-modal__stat-breakdown-value">
                {formatTokenCount(stats.usage.total_output_tokens)}
              </span>
            </div>
          </div>
        </Card>

        {/* Plan Card */}
        <Card className="usage-stats-modal__stat-card">
          <div className="usage-stats-modal__stat-header">
            <BarChart2 size={16} />
            <span>{t('usageStats.plan')}</span>
          </div>
          <div className="usage-stats-modal__stat-value">
            {stats.plan.plan_name}
          </div>
          <div className="usage-stats-modal__stat-meta">
            {t('usageStats.concurrency')}: {stats.key_info.concurrency_limit}
            <br />
            {t('usageStats.rpmLimit')}: {stats.key_info.rpm_limit}
          </div>
        </Card>

        {/* Key Info Card */}
        <Card className="usage-stats-modal__stat-card">
          <div className="usage-stats-modal__stat-header">
            <FileText size={16} />
            <span>{t('usageStats.keyInfo')}</span>
          </div>
          <div className="usage-stats-modal__stat-meta">
            {stats.usage.token_prefix && (
              <>
                {t('usageStats.keyPrefix')}: {stats.usage.token_prefix}...
              </>
            )}
            {stats.usage.created_at && (
              <>
                <br />
                {t('usageStats.createdAt')}: {formatDate(stats.usage.created_at)}
              </>
            )}
            {stats.usage.expires && (
              <>
                <br />
                {t('usageStats.expires')}: {formatDate(stats.usage.expires)}
              </>
            )}
            {stats.usage.blocked !== undefined && (
              <>
                <br />
                <Badge variant={stats.usage.blocked ? 'error' : 'success'}>
                  {stats.usage.blocked ? t('usageStats.blocked') : t('usageStats.active')}
                </Badge>
              </>
            )}
          </div>
        </Card>
      </div>
    );
  };

  const renderLogs = () => {
    if (logsLoading) {
      return (
        <div className="usage-stats-modal__loading">
          <CubeLoading size="large" />
          <span>{t('usageStats.loading')}</span>
        </div>
      );
    }

    if (!logs) return null;

    return (
      <div className="usage-stats-modal__logs">
        <div className="usage-stats-modal__logs-header">
          <span>
            {t('usageStats.totalLogs', { total: logs.total })}
          </span>
          <div className="usage-stats-modal__logs-pagination">
            <Button
              variant="ghost"
              size="small"
              onClick={() => fetchLogs(logsPage - 1)}
              disabled={logsPage <= 1}
            >
              <ChevronLeft size={16} />
            </Button>
            <span>
              {logsPage} / {Math.ceil(logs.total / 20)}
            </span>
            <Button
              variant="ghost"
              size="small"
              onClick={() => fetchLogs(logsPage + 1)}
              disabled={logsPage >= Math.ceil(logs.total / 20)}
            >
              <ChevronRight size={16} />
            </Button>
          </div>
        </div>

        {logs.logs.length === 0 ? (
          <div className="usage-stats-modal__empty">
            {t('usageStats.noLogs')}
          </div>
        ) : (
          <div className="usage-stats-modal__logs-list">
            {logs.logs.map(renderLogEntry)}
          </div>
        )}
      </div>
    );
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={t('usageStats.title')}
      size="large"
      titleExtra={
        <div className="usage-stats-modal__header-actions">
          <Tooltip content={t('usageStats.refresh')}>
            <Button variant="ghost" size="small" onClick={handleRefresh}>
              <RefreshCw size={16} className={loading ? 'spinning' : ''} />
            </Button>
          </Tooltip>
        </div>
      }
    >
      <div className="usage-stats-modal__container">
        <div className="usage-stats-modal__tabs">
          <button
            className={`usage-stats-modal__tab ${activeTab === 'overview' ? 'active' : ''}`}
            onClick={() => setActiveTab('overview')}
          >
            <BarChart2 size={16} />
            {t('usageStats.overview')}
          </button>
          <button
            className={`usage-stats-modal__tab ${activeTab === 'logs' ? 'active' : ''}`}
            onClick={() => setActiveTab('logs')}
          >
            <FileText size={16} />
            {t('usageStats.logs')}
          </button>
        </div>

        <div className="usage-stats-modal__content">
          {activeTab === 'overview' ? renderOverview() : renderLogs()}
        </div>
      </div>
    </Modal>
  );
};