/**
 * Full-screen style modal showing download progress for in-app updates.
 */

import React, { useMemo } from 'react';
import { Modal, Alert } from '@/component-library';
import { Download } from 'lucide-react';
import { useI18n } from '@/infrastructure/i18n';
import { formatBytes } from '@/shared/utils/format';
import type { UpdateDownloadProgressPayload } from './installUpdateWithProgress';
import { formatUpdateInstallError } from './updateErrorMessage';
import './UpdateInstallProgressModal.scss';

export interface UpdateInstallProgressModalProps {
  isOpen: boolean;
  error: string | null;
  progress: UpdateDownloadProgressPayload;
  onCloseError?: () => void;
}

export const UpdateInstallProgressModal: React.FC<UpdateInstallProgressModalProps> = ({
  isOpen,
  error,
  progress,
  onCloseError
}) => {
  const { t } = useI18n('common');
  const { downloaded, total } = progress;
  const pct =
    total != null && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
  const progressText =
    pct != null
      ? t('update.progressPercent', { percent: String(pct) })
      : t('update.progressUnknown');
  const downloadedText = formatBytes(downloaded);
  const totalText = total != null && total > 0 ? formatBytes(total) : null;

  const errorMessage = useMemo(
    () => (error ? formatUpdateInstallError(error, t) : null),
    [error, t]
  );

  return (
    <Modal
      isOpen={isOpen}
      onClose={error ? onCloseError ?? (() => {}) : () => {}}
      title={error ? t('update.downloadFailedTitle') : t('update.downloadingTitle')}
      showCloseButton={!!error}
      size="small"
      contentInset
    >
      <div className="bitfun-update-progress">
        {errorMessage ? (
          <Alert
            type="error"
            message={errorMessage}
            showIcon
            className="bitfun-update-progress__alert"
          />
        ) : (
          <>
            <div className="bitfun-update-progress__hero">
              <div className="bitfun-update-progress__icon" aria-hidden>
                <Download size={18} strokeWidth={2} />
              </div>
              <div className="bitfun-update-progress__hero-copy">
                <p className="bitfun-update-progress__hint">{progressText}</p>
                <p className="bitfun-update-progress__meta">
                  {totalText ? `${downloadedText} / ${totalText}` : downloadedText}
                </p>
              </div>
            </div>

            <div className="bitfun-update-progress__panel">
              <div
                className="bitfun-update-progress__bar"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct ?? undefined}
                aria-label={t('update.downloadingTitle')}
              >
                <div
                  className={
                    pct != null
                      ? 'bitfun-update-progress__fill'
                      : 'bitfun-update-progress__fill bitfun-update-progress__fill--indeterminate'
                  }
                  style={pct != null ? { width: `${pct}%` } : undefined}
                />
              </div>
              <div className="bitfun-update-progress__panel-footer">
                <span className="bitfun-update-progress__panel-label">
                  {t('update.downloadingTitle')}
                </span>
                <span className="bitfun-update-progress__panel-value">{progressText}</span>
              </div>
            </div>

            <p className="bitfun-update-progress__restart">{t('update.restartHint')}</p>
          </>
        )}
      </div>
    </Modal>
  );
};
