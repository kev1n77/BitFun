import React, { useMemo, useState } from 'react';
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Download,
  FolderOpen,
  Loader2,
  Package,
  Plus,
  Puzzle,
  RefreshCw,
  Search as SearchIcon,
  Sparkles,
  Store,
  Trash2,
  TrendingUp,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Badge, Button, ConfirmDialog, Input, Modal, Search, Select } from '@/component-library';
import {
  GalleryDetailModal,
  GalleryEmpty,
  GalleryGrid,
  GalleryLayout,
  GalleryPageHeader,
  GallerySkeleton,
  GalleryZone,
} from '@/app/components';
import type { SkillInfo, SkillLevel, SkillMarketItem } from '@/infrastructure/config/types';
import { getCardGradient } from '@/shared/utils/cardGradients';
import { useInstalledSkills } from './hooks/useInstalledSkills';
import { useSkillMarket } from './hooks/useSkillMarket';
import SkillCard from './components/SkillCard';
import './SkillsScene.scss';
import { useSkillsSceneStore } from './skillsSceneStore';

const SKILLS_SOURCE_URL = 'https://skills.sh';

const SkillsScene: React.FC = () => {
  const { t } = useTranslation('scenes/skills');
  const {
    searchDraft,
    marketQuery,
    installedFilter,
    isAddFormOpen,
    setSearchDraft,
    submitMarketQuery,
    setInstalledFilter,
    setAddFormOpen,
    toggleAddForm,
  } = useSkillsSceneStore();

  const [deleteTarget, setDeleteTarget] = useState<SkillInfo | null>(null);
  const [selectedDetail, setSelectedDetail] = useState<
    | { type: 'installed'; skill: SkillInfo }
    | { type: 'market'; skill: SkillMarketItem }
    | null
  >(null);

  const installed = useInstalledSkills({
    searchQuery: searchDraft,
    activeFilter: installedFilter,
  });

  const installedSkillNames = useMemo(
    () => new Set(installed.skills.map((skill) => skill.name)),
    [installed.skills],
  );

  const market = useSkillMarket({
    searchQuery: marketQuery,
    installedSkillNames,
    onInstalledChanged: async () => {
      await installed.loadSkills(true);
    },
  });

  const isRefreshing = installed.loading || market.marketLoading || market.loadingMore;

  const handleRefreshAll = async () => {
    await Promise.all([
      installed.loadSkills(true),
      market.refresh(),
    ]);
  };

  const handleAddSkill = async () => {
    const added = await installed.handleAdd();
    if (added) {
      setAddFormOpen(false);
      await market.refresh();
    }
  };

  const renderInstalledCard = (skill: SkillInfo, index: number) => (
    <SkillCard
      key={skill.name}
      name={skill.name}
      description={skill.description}
      index={index}
      accentSeed={skill.name}
      badges={(
        <Badge variant={skill.level === 'user' ? 'info' : 'purple'}>
          {skill.level === 'user' ? t('list.item.user') : t('list.item.project')}
        </Badge>
      )}
      actions={[
        {
          id: 'delete',
          icon: <Trash2 size={13} />,
          ariaLabel: t('list.item.deleteTooltip'),
          title: t('list.item.deleteTooltip'),
          tone: 'danger',
          onClick: () => setDeleteTarget(skill),
        },
      ]}
      onOpenDetails={() => setSelectedDetail({ type: 'installed', skill })}
    />
  );

  const renderMarketCard = (skill: SkillMarketItem, index: number) => {
    const isInstalled = installedSkillNames.has(skill.name);
    const isDownloading = market.downloadingPackage === skill.installId;

    return (
      <SkillCard
        key={skill.installId}
        name={skill.name}
        description={skill.description}
        index={index}
        accentSeed={skill.installId}
        iconKind="market"
        badges={isInstalled ? (
          <Badge variant="success">
            <CheckCircle2 size={11} />
            {t('market.item.installed')}
          </Badge>
        ) : null}
        meta={(
          <span className="bitfun-skills-scene__market-meta">
            <TrendingUp size={12} />
            {skill.installs ?? 0}
          </span>
        )}
        actions={[
          {
            id: 'download',
            icon: isInstalled ? <CheckCircle2 size={13} /> : <Download size={13} />,
            ariaLabel: isInstalled ? t('market.item.installed') : t('market.item.downloadProject'),
            title: isDownloading
              ? t('market.item.downloading')
              : (isInstalled ? t('market.item.installedTooltip') : t('market.item.downloadProject')),
            disabled: isDownloading || !market.hasWorkspace || isInstalled,
            tone: isInstalled ? 'success' : 'primary',
            onClick: () => market.handleDownload(skill),
          },
        ]}
        onOpenDetails={() => setSelectedDetail({ type: 'market', skill })}
      />
    );
  };

  const selectedInstalledSkill = selectedDetail?.type === 'installed' ? selectedDetail.skill : null;
  const selectedMarketSkill = selectedDetail?.type === 'market' ? selectedDetail.skill : null;

  return (
    <GalleryLayout className="bitfun-skills-scene">
      <GalleryPageHeader
        title={t('page.title')}
        subtitle={t('page.subtitle')}
        actions={(
          <>
            <Search
              value={searchDraft}
              onChange={setSearchDraft}
              onSearch={submitMarketQuery}
              placeholder={t('page.searchPlaceholder')}
              size="small"
              clearable
              prefixIcon={<></>}
              suffixContent={(
                <button
                  type="button"
                  className="gallery-search-btn"
                  onClick={submitMarketQuery}
                  aria-label={t('page.searchPlaceholder')}
                >
                  <SearchIcon size={14} />
                </button>
              )}
            />
            <button
              type="button"
              className="gallery-action-btn gallery-action-btn--primary"
              onClick={toggleAddForm}
            >
              <Plus size={15} />
              <span>{t('toolbar.addTooltip')}</span>
            </button>
            <button
              type="button"
              className="gallery-action-btn"
              onClick={handleRefreshAll}
              disabled={isRefreshing}
              aria-label={t('toolbar.refreshTooltip')}
              title={t('toolbar.refreshTooltip')}
            >
              <RefreshCw size={15} className={isRefreshing ? 'gallery-spinning' : undefined} />
            </button>
          </>
        )}
      />

      <div className="gallery-zones">
        <GalleryZone
          id="skills-installed-zone"
          title={t('installed.titleAll')}
          subtitle={t('installed.subtitleAll')}
          tools={(
            <>
              <div className="gallery-filter-bar">
                {([
                  ['all', installed.counts.all],
                  ['user', installed.counts.user],
                  ['project', installed.counts.project],
                ] as const).map(([filter, count]) => (
                  <button
                    key={filter}
                    type="button"
                    className={[
                      'gallery-filter-chip',
                      installedFilter === filter && 'is-active',
                    ].filter(Boolean).join(' ')}
                    onClick={() => setInstalledFilter(filter)}
                  >
                    <span>{t(`filters.${filter}`)}</span>
                    <span className="gallery-filter-count">{count}</span>
                  </button>
                ))}
              </div>
              <span className="gallery-zone-count">{installed.filteredSkills.length}</span>
            </>
          )}
        >
          {installed.loading ? <GallerySkeleton /> : null}

          {!installed.loading && installed.error ? (
            <GalleryEmpty
              icon={<Package size={32} strokeWidth={1.5} />}
              message={installed.error}
              isError
            />
          ) : null}

          {!installed.loading && !installed.error && installed.filteredSkills.length === 0 ? (
            <GalleryEmpty
              icon={<Sparkles size={32} strokeWidth={1.5} />}
              message={installed.skills.length === 0 ? t('list.empty.noSkills') : t('list.empty.noMatch')}
            />
          ) : null}

          {!installed.loading && !installed.error && installed.filteredSkills.length > 0 ? (
            <GalleryGrid minCardWidth={360}>
              {installed.filteredSkills.map(renderInstalledCard)}
            </GalleryGrid>
          ) : null}
        </GalleryZone>

        {/* Skill Market Zone - Hidden */}
        {/* <GalleryZone
          id="skills-market-zone"
          title={t('market.title')}
          subtitle={(
            <>
              {t('market.subtitlePrefix')}
              {' '}
              <a href={SKILLS_SOURCE_URL} target="_blank" rel="noreferrer">
                skills.sh
              </a>
              {t('market.subtitleSuffix')}
            </>
          )}
          tools={<span className="gallery-zone-count">{market.totalLoaded}</span>}
        >
          {market.marketLoading ? <GallerySkeleton /> : null}

          {!market.marketLoading && market.loadingMore && market.marketSkills.length === 0
            ? <GallerySkeleton />
            : null}

          {!market.marketLoading && market.marketError ? (
            <GalleryEmpty
              icon={<Store size={32} strokeWidth={1.5} />}
              message={market.marketError}
              isError
            />
          ) : null}

          {!market.marketLoading && !market.loadingMore && !market.marketError && market.marketSkills.length === 0 ? (
            <GalleryEmpty
              icon={<Store size={32} strokeWidth={1.5} />}
              message={marketQuery ? t('market.empty.noMatch') : t('market.empty.noSkills')}
            />
          ) : null}

          {!market.marketLoading && !market.marketError && market.marketSkills.length > 0 ? (
            <GalleryGrid minCardWidth={360}>
              {market.marketSkills.map(renderMarketCard)}
            </GalleryGrid>
          ) : null}

          {!market.marketLoading && !market.marketError && (market.totalPages > 1 || market.hasMore) ? (
            <div className="bitfun-skills-scene__pagination">
              <button
                type="button"
                className="gallery-action-btn"
                onClick={market.goToPrevPage}
                disabled={market.currentPage === 0 || market.loadingMore}
                aria-label={t('market.pagination.prev')}
              >
                <ChevronLeft size={15} />
              </button>
              <span className="bitfun-skills-scene__pagination-info">
                {market.hasMore
                  ? t('market.pagination.infoMore', { current: market.currentPage + 1 })
                  : t('market.pagination.info', {
                    current: market.currentPage + 1,
                    total: market.totalPages,
                  })}
              </span>
              <button
                type="button"
                className="gallery-action-btn"
                onClick={market.goToNextPage}
                disabled={(!market.hasMore && market.currentPage >= market.totalPages - 1) || market.loadingMore}
                aria-label={t('market.pagination.next')}
              >
                {market.loadingMore
                  ? <Loader2 size={15} className="gallery-spinning" />
                  : <ChevronRight size={15} />}
              </button>
            </div>
          ) : null}
        </GalleryZone> */}
      </div>

      <GalleryDetailModal
        isOpen={Boolean(selectedDetail)}
        onClose={() => setSelectedDetail(null)}
        icon={selectedMarketSkill ? <Package size={24} strokeWidth={1.6} /> : <Puzzle size={24} strokeWidth={1.6} />}
        iconGradient={getCardGradient(
          selectedInstalledSkill?.name
          ?? selectedMarketSkill?.installId
          ?? selectedMarketSkill?.name
          ?? 'skill'
        )}
        title={selectedInstalledSkill?.name ?? selectedMarketSkill?.name ?? ''}
        badges={selectedInstalledSkill ? (
          <Badge variant={selectedInstalledSkill.level === 'user' ? 'info' : 'purple'}>
            {selectedInstalledSkill.level === 'user' ? t('list.item.user') : t('list.item.project')}
          </Badge>
        ) : selectedMarketSkill && installedSkillNames.has(selectedMarketSkill.name) ? (
          <Badge variant="success">
            <CheckCircle2 size={11} />
            {t('market.item.installed')}
          </Badge>
        ) : null}
        description={selectedInstalledSkill?.description ?? selectedMarketSkill?.description}
        meta={selectedMarketSkill ? (
          <span className="bitfun-skills-scene__market-meta">
            <TrendingUp size={12} />
            {selectedMarketSkill.installs ?? 0}
          </span>
        ) : null}
        actions={selectedInstalledSkill ? (
          <Button
            variant="danger"
            size="small"
            onClick={() => {
              setDeleteTarget(selectedInstalledSkill);
              setSelectedDetail(null);
            }}
          >
            <Trash2 size={14} />
            {t('deleteModal.delete')}
          </Button>
        ) : selectedMarketSkill ? (
          <Button
            variant={installedSkillNames.has(selectedMarketSkill.name) ? 'secondary' : 'primary'}
            size="small"
            onClick={() => void market.handleDownload(selectedMarketSkill)}
            disabled={
              market.downloadingPackage === selectedMarketSkill.installId
              || !market.hasWorkspace
              || installedSkillNames.has(selectedMarketSkill.name)
            }
          >
            {installedSkillNames.has(selectedMarketSkill.name)
              ? t('market.item.installed')
              : t('market.item.downloadProject')}
          </Button>
        ) : null}
      >
        {selectedInstalledSkill ? (
          <div className="bitfun-skills-scene__detail-row">
            <span className="bitfun-skills-scene__detail-label">{t('list.item.pathLabel')}</span>
            <code className="bitfun-skills-scene__detail-value">{selectedInstalledSkill.path}</code>
          </div>
        ) : null}

        {selectedMarketSkill?.source ? (
          <div className="bitfun-skills-scene__detail-row">
            <span className="bitfun-skills-scene__detail-label">{t('market.item.sourceLabel')}</span>
            <span className="bitfun-skills-scene__detail-value">{selectedMarketSkill.source}</span>
          </div>
        ) : null}

        {selectedMarketSkill ? (
          <div className="bitfun-skills-scene__detail-row">
            <span className="bitfun-skills-scene__detail-label">{t('market.detail.installsLabel')}</span>
            <span className="bitfun-skills-scene__detail-value">{selectedMarketSkill.installs ?? 0}</span>
          </div>
        ) : null}

        {selectedMarketSkill?.url ? (
          <div className="bitfun-skills-scene__detail-row">
            <span className="bitfun-skills-scene__detail-label">{t('market.detail.linkLabel')}</span>
            <a
              href={selectedMarketSkill.url}
              target="_blank"
              rel="noreferrer"
              className="bitfun-skills-scene__detail-link"
            >
              {selectedMarketSkill.url}
            </a>
          </div>
        ) : null}
      </GalleryDetailModal>

      <Modal
        isOpen={isAddFormOpen}
        onClose={() => {
          installed.resetForm();
          setAddFormOpen(false);
        }}
        title={t('form.title')}
        size="small"
      >
        <div className="bitfun-skills-scene__modal-form">
          <Select
            label={t('form.level.label')}
            options={[
              { label: t('form.level.user'), value: 'user' },
              {
                label: `${t('form.level.project')}${installed.hasWorkspace ? '' : t('form.level.projectDisabled')}`,
                value: 'project',
                disabled: !installed.hasWorkspace,
              },
            ]}
            value={installed.formLevel}
            onChange={(value) => installed.setFormLevel(value as SkillLevel)}
            size="medium"
          />

          {installed.formLevel === 'project' && installed.hasWorkspace ? (
            <div className="bitfun-skills-scene__form-hint">
              {t('form.level.currentWorkspace', { path: installed.workspacePath })}
            </div>
          ) : null}

          <div className="bitfun-skills-scene__path-input">
            <Input
              label={t('form.path.label')}
              placeholder={t('form.path.placeholder')}
              value={installed.formPath}
              onChange={(e) => installed.setFormPath(e.target.value)}
              variant="outlined"
            />
            <button
              type="button"
              className="gallery-action-btn"
              onClick={installed.handleBrowse}
              aria-label={t('form.path.browseTooltip')}
            >
              <FolderOpen size={15} />
            </button>
          </div>
          <div className="bitfun-skills-scene__path-hint">
            {t('form.path.hint')}
          </div>

          {installed.isValidating ? (
            <div className="bitfun-skills-scene__validating">{t('form.validating')}</div>
          ) : null}

          {installed.validationResult ? (
            <div
              className={[
                'bitfun-skills-scene__validation',
                installed.validationResult.valid ? 'is-valid' : 'is-invalid',
              ].filter(Boolean).join(' ')}
            >
              {installed.validationResult.valid ? (
                <>
                  <div className="bitfun-skills-scene__validation-name">
                    {installed.validationResult.name}
                  </div>
                  <div className="bitfun-skills-scene__validation-desc">
                    {installed.validationResult.description}
                  </div>
                </>
              ) : (
                <div className="bitfun-skills-scene__validation-error">
                  {installed.validationResult.error}
                </div>
              )}
            </div>
          ) : null}

          <div className="bitfun-skills-scene__modal-form-actions">
            <Button
              variant="secondary"
              size="small"
              onClick={() => {
                installed.resetForm();
                setAddFormOpen(false);
              }}
            >
              {t('form.actions.cancel')}
            </Button>
            <Button
              variant="primary"
              size="small"
              onClick={handleAddSkill}
              disabled={!installed.validationResult?.valid || installed.isAdding}
            >
              {installed.isAdding ? t('form.actions.adding') : t('form.actions.add')}
            </Button>
          </div>
        </div>
      </Modal>

      <ConfirmDialog
        isOpen={Boolean(deleteTarget)}
        onClose={() => setDeleteTarget(null)}
        onConfirm={async () => {
          if (!deleteTarget) {
            return;
          }
          const deleted = await installed.handleDelete(deleteTarget);
          if (deleted) {
            setDeleteTarget(null);
          }
        }}
        title={t('deleteModal.title')}
        message={t('deleteModal.message', { name: deleteTarget?.name ?? '' })}
        type="warning"
        confirmDanger
        confirmText={t('deleteModal.delete')}
        cancelText={t('deleteModal.cancel')}
      />
    </GalleryLayout>
  );
};

export default SkillsScene;
