import React from 'react';
import { createPortal } from 'react-dom';
import { Button } from '@/component-library';
import { useI18n } from '@/infrastructure/i18n';
import type { CheckForUpdatesResponse } from '@/infrastructure/api/service-api/SystemAPI';
import { ArrowRight, Download, X } from 'lucide-react';
import './DailyUpdatePromptToast.scss';

export interface DailyUpdatePromptToastProps {
  isOpen: boolean;
  data: CheckForUpdatesResponse | null;
  onLater: () => void;
  onSkip?: () => void;
  onInstall: () => void;
}

export const DailyUpdatePromptToast: React.FC<DailyUpdatePromptToastProps> = ({
  isOpen,
  data,
  onLater,
  onSkip,
  onInstall
}) => {
  const { t } = useI18n('common');

  if (!isOpen || !data?.updateAvailable || typeof document === 'undefined') {
    return null;
  }

  const latest = data.latestVersion ?? '';
  const notes = data.releaseNotes?.trim();

  return createPortal(
    <aside
      className="bitfun-daily-update-toast"
      role="dialog"
      aria-modal="false"
      aria-labelledby="bitfun-daily-update-toast-title"
      aria-describedby="bitfun-daily-update-toast-subtitle"
    >
      <div className="bitfun-daily-update-toast__accent" aria-hidden />
      <button
        className="bitfun-daily-update-toast__close"
        type="button"
        onClick={onLater}
        aria-label={t('actions.close')}
      >
        <X size={16} strokeWidth={2} />
      </button>

      <div className="bitfun-daily-update-toast__header">
        <div className="bitfun-daily-update-toast__icon" aria-hidden>
          <Download size={18} strokeWidth={2} />
        </div>
        <div className="bitfun-daily-update-toast__header-copy">
          <div
            className="bitfun-daily-update-toast__eyebrow"
            id="bitfun-daily-update-toast-title"
          >
            {t('update.availableTitle')}
          </div>
          <p
            className="bitfun-daily-update-toast__subtitle"
            id="bitfun-daily-update-toast-subtitle"
          >
            {t('update.availableSubtitle')}
          </p>
        </div>
      </div>

      <div className="bitfun-daily-update-toast__versions" aria-label={t('update.availableTitle')}>
        <div className="bitfun-daily-update-toast__version">
          <span className="bitfun-daily-update-toast__version-label">
            {t('update.currentVersion')}
          </span>
          <span className="bitfun-daily-update-toast__version-value">{data.currentVersion}</span>
        </div>
        <div className="bitfun-daily-update-toast__version-arrow" aria-hidden>
          <ArrowRight size={14} strokeWidth={2} />
        </div>
        <div className="bitfun-daily-update-toast__version bitfun-daily-update-toast__version--latest">
          <span className="bitfun-daily-update-toast__version-label">
            {t('update.latestVersion')}
          </span>
          <span className="bitfun-daily-update-toast__version-value">{latest}</span>
        </div>
      </div>

      {notes ? (
        <div className="bitfun-daily-update-toast__notes">
          <div className="bitfun-daily-update-toast__notes-label">
            {t('update.releaseNotes')}
          </div>
          <pre className="bitfun-daily-update-toast__notes-body">{notes}</pre>
        </div>
      ) : null}

      <div className="bitfun-daily-update-toast__actions">
        <Button variant="secondary" size="small" onClick={onLater}>
          {t('update.later')}
        </Button>
        {onSkip ? (
          <Button variant="ghost" size="small" onClick={onSkip}>
            {t('update.skipVersion')}
          </Button>
        ) : null}
        <Button variant="primary" size="small" onClick={onInstall}>
          {t('update.install')}
        </Button>
      </div>
    </aside>,
    document.body
  );
};
