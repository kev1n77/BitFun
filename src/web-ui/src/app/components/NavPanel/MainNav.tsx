/**
 * MainNav — default workspace navigation sidebar.
 *
 * Layout (top to bottom):
 *   1. Top: New sessions | Extensions (expand → Agents | Skills)
 *   2. Workspace
 *   3. Bottom: MiniApp
 *
 * When a scene-nav transition is active (`isDeparting=true`), items receive
 * positional CSS classes for the split-open animation effect.
 */

import React, { useCallback, useState, useMemo, useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { Plus, FolderOpen, FolderPlus, History, Check, Users, Puzzle, Blocks, ChevronDown } from 'lucide-react';
import { Tooltip } from '@/component-library';
import { useApp } from '../../hooks/useApp';
import { useSceneManager } from '../../hooks/useSceneManager';
import { useI18n } from '@/infrastructure/i18n/hooks/useI18n';
import type { SceneTabId } from '../SceneBar/types';
import SectionHeader from './components/SectionHeader';
import MiniAppEntry from './components/MiniAppEntry';
import WorkspaceListSection from './sections/workspaces/WorkspaceListSection';
import { useSceneStore } from '../../stores/sceneStore';
import { useMiniAppCatalogSync } from '../../scenes/miniapps/hooks/useMiniAppCatalogSync';
import { flowChatStore } from '@/flow_chat/store/FlowChatStore';
import { flowChatManager } from '@/flow_chat/services/FlowChatManager';
import { workspaceManager } from '@/infrastructure/services/business/workspaceManager';
import { useWorkspaceContext } from '@/infrastructure/contexts/WorkspaceContext';
import { createLogger } from '@/shared/utils/logger';
import { useSSHRemoteContext, SSHConnectionDialog, RemoteFileBrowser } from '@/features/ssh-remote';
import { useSessionModeStore } from '../../stores/sessionModeStore';

import './NavPanel.scss';

const log = createLogger('MainNav');

interface MainNavProps {
  isDeparting?: boolean;
  anchorNavSceneId?: SceneTabId | null;
}

const MainNav: React.FC<MainNavProps> = ({
  isDeparting: _isDeparting = false,
  anchorNavSceneId: _anchorNavSceneId = null,
}) => {
  useMiniAppCatalogSync();

  const sshRemote = useSSHRemoteContext();
  const [isSSHConnectionDialogOpen, setIsSSHConnectionDialogOpen] = useState(false);

  useEffect(() => {
    if (sshRemote.showFileBrowser) {
      setIsSSHConnectionDialogOpen(false);
    }
  }, [sshRemote.showFileBrowser]);

  const { switchLeftPanelTab } = useApp();
  const { openScene } = useSceneManager();
  const activeTabId = useSceneStore(s => s.activeTabId);
  const { t } = useI18n('common');
  const {
    currentWorkspace,
    recentWorkspaces,
    openedWorkspacesList,
    switchWorkspace,
  } = useWorkspaceContext();

  const activeMiniAppId = useMemo(
    () => (typeof activeTabId === 'string' && activeTabId.startsWith('miniapp:') ? activeTabId.slice('miniapp:'.length) : null),
    [activeTabId]
  );

  // Section expand state
  const [expandedSections, setExpandedSections] = useState<Set<string>>(
    () => new Set(['workspace'])
  );

  const workspaceMenuButtonRef = useRef<HTMLButtonElement | null>(null);
  const workspaceMenuRef = useRef<HTMLDivElement | null>(null);
  const [workspaceMenuOpen, setWorkspaceMenuOpen] = useState(false);
  const [workspaceMenuClosing, setWorkspaceMenuClosing] = useState(false);
  const [workspaceMenuPos, setWorkspaceMenuPos] = useState({ top: 0, left: 0 });
  const [isExtensionsOpen, setIsExtensionsOpen] = useState(false);

  const toggleSection = useCallback((id: string) => {
    setExpandedSections(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }, []);

  const closeWorkspaceMenu = useCallback(() => {
    setWorkspaceMenuClosing(true);
    window.setTimeout(() => {
      setWorkspaceMenuOpen(false);
      setWorkspaceMenuClosing(false);
    }, 150);
  }, []);

  const openWorkspaceMenu = useCallback(async () => {
    try {
      await workspaceManager.cleanupInvalidWorkspaces();
    } catch (error) {
      log.warn('Failed to cleanup invalid workspaces before opening workspace menu', { error });
    }
    const rect = workspaceMenuButtonRef.current?.getBoundingClientRect();
    if (!rect) return;
    setWorkspaceMenuPos({ top: rect.bottom + 6, left: rect.left });
    setWorkspaceMenuOpen(true);
    setWorkspaceMenuClosing(false);
  }, []);

  const toggleWorkspaceMenu = useCallback(() => {
    if (workspaceMenuOpen) { closeWorkspaceMenu(); return; }
    void openWorkspaceMenu();
  }, [closeWorkspaceMenu, openWorkspaceMenu, workspaceMenuOpen]);

  const setSessionMode = useSessionModeStore(s => s.setMode);

  useEffect(() => {
    openedWorkspacesList.forEach(workspace => {
      void flowChatStore.initializeFromDisk(workspace.rootPath);
    });
  }, [openedWorkspacesList]);

  const handleCreateSession = useCallback(async (mode?: 'agentic' | 'Cowork') => {
    openScene('session');
    switchLeftPanelTab('sessions');
    try {
      await flowChatManager.createChatSession(
        {},
        mode ?? 'agentic'
      );
    } catch (err) {
      log.error('Failed to create session', err);
    }
  }, [openScene, switchLeftPanelTab]);

  const handleCreateCodeSession = useCallback(() => {
    setSessionMode('code');
    void handleCreateSession('agentic');
  }, [handleCreateSession, setSessionMode]);

  const handleCreateCoworkSession = useCallback(() => {
    setSessionMode('cowork');
    void handleCreateSession('Cowork');
  }, [handleCreateSession, setSessionMode]);

  const handleOpenProject = useCallback(async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: t('header.selectProjectDirectory') });
      if (selected && typeof selected === 'string') {
        await workspaceManager.openWorkspace(selected);
      }
    } catch (err) {
      log.error('Failed to open project', err);
    }
  }, [t]);

  const handleNewProject = useCallback(() => {
    window.dispatchEvent(new Event('nav:new-project'));
  }, []);

  const handleSwitchWorkspace = useCallback(async (workspaceId: string) => {
    const targetWorkspace = recentWorkspaces.find(item => item.id === workspaceId);
    if (!targetWorkspace) return;
    closeWorkspaceMenu();
    await switchWorkspace(targetWorkspace);
  }, [closeWorkspaceMenu, recentWorkspaces, switchWorkspace]);

  const handleOpenRemoteSSH = useCallback(() => {
    closeWorkspaceMenu();
    setIsSSHConnectionDialogOpen(true);
  }, [closeWorkspaceMenu]);

  const handleSelectRemoteWorkspace = useCallback(async (path: string) => {
    try {
      await sshRemote.openWorkspace(path);
      sshRemote.setShowFileBrowser(false);
      setIsSSHConnectionDialogOpen(false);
    } catch (err) {
      log.error('Failed to open remote workspace', err);
    }
  }, [sshRemote]);

  useEffect(() => {
    if (!workspaceMenuOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (workspaceMenuButtonRef.current?.contains(target)) return;
      if (workspaceMenuRef.current?.contains(target)) return;
      closeWorkspaceMenu();
    };
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeWorkspaceMenu();
    };
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [closeWorkspaceMenu, workspaceMenuOpen]);

  const handleOpenAgents = useCallback(() => {
    openScene('agents');
  }, [openScene]);

  const handleOpenSkills = useCallback(() => {
    openScene('skills');
  }, [openScene]);

  const isAgentsActive = activeTabId === 'agents';
  const isSkillsActive = activeTabId === 'skills';

  useEffect(() => {
    if (isAgentsActive || isSkillsActive) {
      setIsExtensionsOpen(true);
    }
  }, [isAgentsActive, isSkillsActive]);

  const workspaceMenuPortal = workspaceMenuOpen ? createPortal(
    <div
      ref={workspaceMenuRef}
      className={`bitfun-nav-panel__workspace-menu${workspaceMenuClosing ? ' is-closing' : ''}`}
      role="menu"
      style={{ top: workspaceMenuPos.top, left: workspaceMenuPos.left }}
    >
      <button
        type="button"
        className="bitfun-nav-panel__workspace-menu-item"
        role="menuitem"
        onClick={() => { closeWorkspaceMenu(); void handleOpenProject(); }}
      >
        <FolderOpen size={13} />
        <span>{t('header.openProject')}</span>
      </button>
      <button
        type="button"
        className="bitfun-nav-panel__workspace-menu-item"
        role="menuitem"
        onClick={() => { closeWorkspaceMenu(); handleNewProject(); }}
      >
        <FolderPlus size={13} />
        <span>{t('header.newProject')}</span>
      </button>
      <button
        type="button"
        className="bitfun-nav-panel__workspace-menu-item"
        role="menuitem"
        onClick={handleOpenRemoteSSH}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
          <path d="M9 3H5a2 2 0 0 0-2 2v4m6-6h10a2 2 0 0 1 2 2v4M9 3v18m0 0h10a2 2 0 0 0 2-2v-4M9 21H5a2 2 0 0 1-2-2v-4m0-6v6" />
        </svg>
        <span>{t('ssh.remote.connect')}</span>
      </button>
      <div className="bitfun-nav-panel__workspace-menu-divider" role="separator" />
      <div className="bitfun-nav-panel__workspace-menu-section-title">
        <History size={12} aria-hidden="true" />
        <span>{t('header.recentWorkspaces')}</span>
      </div>
      {recentWorkspaces.length === 0 ? (
        <div className="bitfun-nav-panel__workspace-menu-empty">
          <span>{t('header.noRecentWorkspaces')}</span>
        </div>
      ) : (
        <div className="bitfun-nav-panel__workspace-menu-workspaces">
          {recentWorkspaces.map((workspace) => (
            <button
              key={workspace.id}
              type="button"
              className="bitfun-nav-panel__workspace-menu-item bitfun-nav-panel__workspace-menu-item--workspace"
              role="menuitem"
              title={workspace.rootPath}
              onClick={() => { void handleSwitchWorkspace(workspace.id); }}
            >
              <FolderOpen size={13} aria-hidden="true" />
              <span className="bitfun-nav-panel__workspace-menu-item-main">{workspace.name}</span>
              {workspace.id === currentWorkspace?.id ? <Check size={12} aria-hidden="true" /> : null}
            </button>
          ))}
        </div>
      )}
    </div>,
    document.body
  ) : null;

  const createCodeTooltip = t('nav.sessions.newCodeSession');
  const createCoworkTooltip = t('nav.sessions.newCoworkSession');
  const openProjectTooltip = t('header.openProject');
  const agentsTooltip = t('nav.tooltips.agents');
  const skillsTooltip = t('nav.tooltips.skills');
  const extensionsLabel = t('nav.sections.extensions');
  return (
    <>
      {/* ── Top action strip ────────────────────────── */}
      <div className="bitfun-nav-panel__top-actions">
        <Tooltip content={createCodeTooltip} placement="right" followCursor>
          <button
            type="button"
            className="bitfun-nav-panel__top-action-btn"
            onClick={handleCreateCodeSession}
            aria-label={createCodeTooltip}
          >
            <span className="bitfun-nav-panel__top-action-icon-circle" aria-hidden="true">
              <Plus size={12} />
            </span>
            <span>{t('nav.sessions.newCodeSessionShort')}</span>
          </button>
        </Tooltip>

        <Tooltip content={createCoworkTooltip} placement="right" followCursor>
          <button
            type="button"
            className="bitfun-nav-panel__top-action-btn"
            onClick={handleCreateCoworkSession}
            aria-label={createCoworkTooltip}
          >
            <span className="bitfun-nav-panel__top-action-icon-circle" aria-hidden="true">
              <Plus size={12} />
            </span>
            <span>{t('nav.sessions.newCoworkSessionShort')}</span>
          </button>
        </Tooltip>

        <div className="bitfun-nav-panel__top-action-expand">
          <Tooltip content={extensionsLabel} placement="right" followCursor>
            <button
              type="button"
              className={[
                'bitfun-nav-panel__top-action-btn',
                'bitfun-nav-panel__top-action-btn--expand',
                isExtensionsOpen ? 'is-open' : '',
              ].filter(Boolean).join(' ')}
              onClick={() => setIsExtensionsOpen(v => !v)}
              aria-expanded={isExtensionsOpen}
              aria-label={extensionsLabel}
            >
              <span className="bitfun-nav-panel__top-action-expand-icons" aria-hidden="true">
                <Blocks size={15} className="bitfun-nav-panel__top-action-expand-icon-default" />
                <ChevronDown
                  size={15}
                  className={[
                    'bitfun-nav-panel__top-action-expand-icon-chevron',
                    isExtensionsOpen ? 'is-open' : '',
                  ].filter(Boolean).join(' ')}
                />
              </span>
              <span>{extensionsLabel}</span>
            </button>
          </Tooltip>

          <div className={`bitfun-nav-panel__top-action-sublist${isExtensionsOpen ? ' is-open' : ''}`}>
            <Tooltip content={agentsTooltip} placement="right" followCursor>
              <button
                type="button"
                className={[
                  'bitfun-nav-panel__top-action-btn',
                  'bitfun-nav-panel__top-action-btn--sub',
                  isAgentsActive ? 'is-active' : '',
                ].filter(Boolean).join(' ')}
                onClick={handleOpenAgents}
                aria-label={agentsTooltip}
              >
                <span className="bitfun-nav-panel__top-action-icon-slot" aria-hidden="true">
                  <Users size={15} />
                </span>
                <span>{t('nav.items.agents')}</span>
              </button>
            </Tooltip>

            <Tooltip content={skillsTooltip} placement="right" followCursor>
              <button
                type="button"
                className={[
                  'bitfun-nav-panel__top-action-btn',
                  'bitfun-nav-panel__top-action-btn--sub',
                  isSkillsActive ? 'is-active' : '',
                ].filter(Boolean).join(' ')}
                onClick={handleOpenSkills}
                aria-label={skillsTooltip}
              >
                <span className="bitfun-nav-panel__top-action-icon-slot" aria-hidden="true">
                  <Puzzle size={15} />
                </span>
                <span>{t('nav.items.skills')}</span>
              </button>
            </Tooltip>
          </div>
        </div>
      </div>

      {/* ── Sections ────────────────────────────────── */}
      <div className="bitfun-nav-panel__sections">

        {/* Workspace */}
        <div className="bitfun-nav-panel__section">
          <SectionHeader
            label={t('nav.sections.workspace')}
            collapsible
            isOpen={expandedSections.has('workspace')}
            onToggle={() => toggleSection('workspace')}
            actions={
              <div className="bitfun-nav-panel__workspace-action-wrap">
                <Tooltip content={openProjectTooltip} placement="right" followCursor disabled={workspaceMenuOpen}>
                  <button
                    ref={workspaceMenuButtonRef}
                    type="button"
                    className={`bitfun-nav-panel__section-action${workspaceMenuOpen ? ' is-active' : ''}`}
                    aria-label={openProjectTooltip}
                    aria-expanded={workspaceMenuOpen}
                    onClick={toggleWorkspaceMenu}
                  >
                    <Plus size={13} />
                  </button>
                </Tooltip>
              </div>
            }
          />
          <div className={`bitfun-nav-panel__collapsible${expandedSections.has('workspace') ? '' : ' is-collapsed'}`}>
            <div className="bitfun-nav-panel__collapsible-inner">
              <div className="bitfun-nav-panel__items">
                <WorkspaceListSection variant="projects" />
              </div>
            </div>
          </div>
        </div>

      </div>

      {/* ── Bottom: MiniApp ───────────────────────── */}
      <div className="bitfun-nav-panel__bottom-bar">
        <div className="bitfun-nav-panel__miniapp-footer">
          <MiniAppEntry
            isActive={activeTabId === 'miniapps' || !!activeMiniAppId}
            activeMiniAppId={activeMiniAppId}
            onOpenMiniApps={() => openScene('miniapps')}
            onOpenMiniApp={(appId) => openScene(`miniapp:${appId}`)}
          />
        </div>
      </div>

      {workspaceMenuPortal}

      {/* SSH Remote Dialogs */}
      <SSHConnectionDialog
        open={isSSHConnectionDialogOpen}
        onClose={() => setIsSSHConnectionDialogOpen(false)}
      />
      {sshRemote.showFileBrowser && sshRemote.connectionId && (
        <RemoteFileBrowser
          connectionId={sshRemote.connectionId}
          onSelect={handleSelectRemoteWorkspace}
          onCancel={() => {
            sshRemote.setShowFileBrowser(false);
            void sshRemote.disconnect();
          }}
        />
      )}
    </>
  );
};

export default MainNav;
