/**
 * About dialog component.
 * Shows app version and license info.
 * Uses component library Modal.
 */

import React from 'react';
import { useI18n } from '@/infrastructure/i18n';
import { Modal } from '@/component-library';
import {
  getAboutInfo,
  formatBuildDate
} from '@/shared/utils/version';
import './AboutDialog.scss';

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

  const aboutInfo = getAboutInfo();
  const { version } = aboutInfo;

  return (
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
          <div className="bitfun-about-dialog__dots">
            <span></span>
            <span></span>
            <span></span>
          </div>
        </div>

        {/* Scrollable area */}
        <div className="bitfun-about-dialog__scrollable">
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
  );
};

export default AboutDialog;
