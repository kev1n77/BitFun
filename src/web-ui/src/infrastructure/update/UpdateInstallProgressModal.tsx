/**
 * Full-screen style modal showing download progress for in-app updates.
 */

import React, { useMemo } from 'react';
import { Modal, Alert } from '@/component-library';
import { useI18n } from '@/infrastructure/i18n';
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
            <p className="bitfun-update-progress__hint">
              {pct != null
                ? t('update.progressPercent', { percent: String(pct) })
                : t('update.progressUnknown')}
            </p>
            <p className="bitfun-update-progress__restart">{t('update.restartHint')}</p>
          </>
        )}
      </div>
    </Modal>
  );
};
