/**
 * About dialog component.
 * Shows app version and update info.
 * Uses component library Modal.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useI18n } from '@/infrastructure/i18n';
import { Modal, Button } from '@/component-library';
import { Check, Download, CheckCircle2, AlertCircle } from 'lucide-react';
import {
  getAboutInfo,
  formatBuildDate
} from '@/shared/utils/version';
import { createLogger } from '@/shared/utils/logger';
import { systemAPI } from '@/infrastructure/api';
import type { CheckForUpdatesResponse } from '@/infrastructure/api/service-api/SystemAPI';
import { isTauriRuntime } from '@/infrastructure/update/tauriEnv';
import { installUpdateWithProgress } from '@/infrastructure/update/installUpdateWithProgress';
import { UpdateAvailableDialog } from '@/infrastructure/update/UpdateAvailableDialog';
import { UpdateInstallProgressModal } from '@/infrastructure/update/UpdateInstallProgressModal';
import { formatUpdateInstallError } from '@/infrastructure/update/updateErrorMessage';
import './AboutDialog.scss';

const log = createLogger('AboutDialog');

interface AboutDialogProps {
  /** Whether visible */
  isOpen: boolean;
  /** Close callback */
  onClose: () => void;
}

export const AboutDialog: React.FC<AboutDialogProps> = ({
  isOpen,
  onClose
}) => {
  const { t } = useI18n('common');
  const [manualCheckBusy, setManualCheckBusy] = useState(false);
  const [manualCheckStatus, setManualCheckStatus] = useState<'idle' | 'latest' | 'error'>('idle');
  const [manualCheckErrorMessage, setManualCheckErrorMessage] = useState<string | null>(null);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualData, setManualData] = useState<CheckForUpdatesResponse | null>(null);
  const [progressOpen, setProgressOpen] = useState(false);
  const [progress, setProgress] = useState<{ downloaded: number; total: number | null }>({
    downloaded: 0,
    total: null
  });
  const [installError, setInstallError] = useState<string | null>(null);

  const aboutInfo = getAboutInfo();
  const { version } = aboutInfo;

  useEffect(() => {
    if (isOpen) {
      setManualCheckStatus('idle');
      setManualCheckErrorMessage(null);
    }
  }, [isOpen]);

  const handleCheckForUpdates = useCallback(async () => {
    if (!isTauriRuntime()) {
      return;
    }
    setManualCheckStatus('idle');
    setManualCheckErrorMessage(null);
    setManualCheckBusy(true);
    try {
      const res = await systemAPI.checkForUpdates();
      if (!res.updateAvailable) {
        setManualCheckStatus('latest');
      } else {
        setManualData(res);
        setManualOpen(true);
      }
    } catch (e) {
      log.error('check_for_updates failed', e);
      const msg = e instanceof Error ? e.message : String(e);
      setManualCheckErrorMessage(formatUpdateInstallError(msg, t));
      setManualCheckStatus('error');
    } finally {
      setManualCheckBusy(false);
    }
  }, [t]);

  const onManualLater = useCallback(() => {
    setManualOpen(false);
    setManualData(null);
  }, []);

  const onManualInstall = useCallback(async () => {
    setManualOpen(false);
    setManualData(null);
    setInstallError(null);
    setProgress({ downloaded: 0, total: null });
    setProgressOpen(true);
    try {
      await installUpdateWithProgress(next => {
        setProgress(next);
      });
      setProgressOpen(false);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setInstallError(msg);
    }
  }, []);

  const onCloseProgressError = useCallback(() => {
    setProgressOpen(false);
    setInstallError(null);
  }, []);

  return (
    <>
      <Modal
        isOpen={isOpen}
        onClose={onClose}
        title={t('header.about')}
        showCloseButton={true}
        size="medium"
      >
        <div className="bitfun-about-dialog__content">
          {/* Hero section - product info */}
          <div className="bitfun-about-dialog__hero">
            <h1 className="bitfun-about-dialog__title">{version.name}</h1>
            <div className="bitfun-about-dialog__version-badge">
              {t('about.customVersion')}
            </div>
            <div className="bitfun-about-dialog__divider" />
            <div className="bitfun-about-dialog__version-line">
              {version.version}
            </div>
          </div>

          {/* Scrollable area */}
          <div className="bitfun-about-dialog__scrollable">
            {isTauriRuntime() ? (
              <div className="bitfun-about-dialog__update-card">
                <div className="bitfun-about-dialog__update-card-head">
                  <div className="bitfun-about-dialog__update-card-icon" aria-hidden>
                    <Download size={18} strokeWidth={2} />
                  </div>
                  <div className="bitfun-about-dialog__update-card-meta">
                    <div className="bitfun-about-dialog__update-card-title">
                      {t('about.updateSectionTitle')}
                    </div>
                    <p className="bitfun-about-dialog__update-card-hint">
                      {t('about.updateSectionHint')}
                    </p>
                  </div>
                </div>
                <div className="bitfun-about-dialog__update-card-actions-row">
                  <div className="bitfun-about-dialog__update-card-status-slot">
                    {manualCheckStatus === 'latest' ? (
                      <div
                        className="bitfun-about-dialog__update-status bitfun-about-dialog__update-status--success"
                        role="status"
                      >
                        <CheckCircle2 size={14} aria-hidden />
                        <span>{t('update.noUpdate')}</span>
                      </div>
                    ) : null}
                    {manualCheckStatus === 'error' && manualCheckErrorMessage ? (
                      <div
                        className="bitfun-about-dialog__update-status bitfun-about-dialog__update-status--error"
                        role="alert"
                      >
                        <AlertCircle size={14} aria-hidden />
                        <span>{manualCheckErrorMessage}</span>
                      </div>
                    ) : null}
                  </div>
                  <div className="bitfun-about-dialog__update-card-actions">
                    <Button
                      variant="secondary"
                      size="small"
                      isLoading={manualCheckBusy}
                      onClick={() => void handleCheckForUpdates()}
                    >
                      {!manualCheckBusy ? (
                        <Check size={14} className="bitfun-about-dialog__update-btn-icon" aria-hidden />
                      ) : null}
                      {manualCheckBusy ? t('update.checking') : t('update.checkForUpdates')}
                    </Button>
                  </div>
                </div>
              </div>
            ) : (
              <p className="bitfun-about-dialog__update-hint">{t('update.desktopOnly')}</p>
            )}
            <div className="bitfun-about-dialog__info-section">
              <div className="bitfun-about-dialog__info-card">
                <div className="bitfun-about-dialog__info-row">
                  <span className="bitfun-about-dialog__info-label">{t('about.buildDate')}</span>
                  <span className="bitfun-about-dialog__info-value">
                    {formatBuildDate(version.buildDate)}
                  </span>
                </div>
              </div>
            </div>
          </div>

          {/* Footer */}
          <div className="bitfun-about-dialog__footer">
            <p className="bitfun-about-dialog__copyright">
              {t('about.supportPrefix')}<strong>{t('about.supportLab')}</strong>{t('about.supportSuffix')}
            </p>
          </div>
        </div>
      </Modal>

      <UpdateAvailableDialog
        isOpen={manualOpen}
        variant="manual"
        data={manualData}
        onLater={onManualLater}
        onInstall={onManualInstall}
      />
      <UpdateInstallProgressModal
        isOpen={progressOpen}
        error={installError}
        progress={progress}
        onCloseError={onCloseProgressError}
      />
    </>
  );
};

export default AboutDialog;
