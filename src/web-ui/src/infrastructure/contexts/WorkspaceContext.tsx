 

import React, { createContext, useContext, useEffect, useState, useCallback, useRef, ReactNode, useMemo } from 'react';
import { workspaceManager, WorkspaceState, WorkspaceEvent } from '../services/business/workspaceManager';
import { WorkspaceInfo, WorkspaceKind } from '../../shared/types';
import { createLogger } from '@/shared/utils/logger';

const log = createLogger('WorkspaceProvider');

const isVisibleWorkspace = (
  workspace: WorkspaceInfo | null | undefined
): workspace is WorkspaceInfo => {
  return !!workspace && workspace.workspaceKind !== WorkspaceKind.Assistant;
};

const getWorkspaceDisplayName = (workspace: WorkspaceInfo | null): string => {
  if (!workspace) {
    return '';
  }

  return workspace.name;
};

interface WorkspaceContextValue extends WorkspaceState {
  activeWorkspace: WorkspaceInfo | null;
  openedWorkspacesList: WorkspaceInfo[];
  normalWorkspacesList: WorkspaceInfo[];
  assistantWorkspacesList: WorkspaceInfo[];
  openWorkspace: (path: string) => Promise<WorkspaceInfo>;
  createAssistantWorkspace: () => Promise<WorkspaceInfo>;
  closeWorkspace: () => Promise<void>;
  closeWorkspaceById: (workspaceId: string) => Promise<void>;
  deleteAssistantWorkspace: (workspaceId: string) => Promise<void>;
  resetAssistantWorkspace: (workspaceId: string) => Promise<WorkspaceInfo>;
  switchWorkspace: (workspace: WorkspaceInfo) => Promise<WorkspaceInfo>;
  setActiveWorkspace: (workspaceId: string) => Promise<WorkspaceInfo>;
  reorderOpenedWorkspacesInSection: (
    section: 'assistants' | 'projects',
    sourceWorkspaceId: string,
    targetWorkspaceId: string,
    position: 'before' | 'after'
  ) => Promise<void>;
  scanWorkspaceInfo: () => Promise<WorkspaceInfo | null>;
  refreshRecentWorkspaces: () => Promise<void>;
  hasWorkspace: boolean;
  workspaceName: string;
  workspacePath: string;
}

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

interface WorkspaceProviderProps {
  children: ReactNode;
}

export const WorkspaceProvider: React.FC<WorkspaceProviderProps> = ({ children }) => {
  const [state, setState] = useState<WorkspaceState>(() => {
    try {
      return workspaceManager.getState();
    } catch (error) {
      log.warn('WorkspaceManager not initialized, using default state', error);
      return {
        currentWorkspace: null,
        openedWorkspaces: new Map(),
        activeWorkspaceId: null,
        lastUsedWorkspaceId: null,
        recentWorkspaces: [],
        loading: false,
        error: null,
      };
    }
  });

  const isInitializedRef = useRef(false);

  useEffect(() => {
    const removeListener = workspaceManager.addEventListener((_event: WorkspaceEvent) => {
      // Workspace metadata such as identity/name can change without affecting ids or list lengths.
      // Always sync the latest manager state so React consumers re-render for these updates.
      setState(workspaceManager.getState());
    });

    return () => {
      removeListener();
    };
  }, []);

  useEffect(() => {
    const initializeWorkspace = async () => {
      if (isInitializedRef.current) {
        return;
      }

      try {
        isInitializedRef.current = true;
        setState(prev => ({ ...prev, loading: true }));
        await workspaceManager.initialize();
        setState(workspaceManager.getState());
      } catch (error) {
        log.error('Failed to initialize workspace state', error);
        isInitializedRef.current = false;
        setState(prev => ({ ...prev, loading: false, error: String(error) }));
      }
    };

    initializeWorkspace();
  }, []);

  const openWorkspace = useCallback(async (path: string): Promise<WorkspaceInfo> => {
    return await workspaceManager.openWorkspace(path);
  }, []);

  const createAssistantWorkspace = useCallback(async (): Promise<WorkspaceInfo> => {
    return await workspaceManager.createAssistantWorkspace();
  }, []);

  const closeWorkspace = useCallback(async (): Promise<void> => {
    return await workspaceManager.closeWorkspace();
  }, []);

  const closeWorkspaceById = useCallback(async (workspaceId: string): Promise<void> => {
    return await workspaceManager.closeWorkspaceById(workspaceId);
  }, []);

  const deleteAssistantWorkspace = useCallback(async (workspaceId: string): Promise<void> => {
    return await workspaceManager.deleteAssistantWorkspace(workspaceId);
  }, []);

  const resetAssistantWorkspace = useCallback(async (workspaceId: string): Promise<WorkspaceInfo> => {
    return await workspaceManager.resetAssistantWorkspace(workspaceId);
  }, []);

  const switchWorkspace = useCallback(async (workspace: WorkspaceInfo): Promise<WorkspaceInfo> => {
    return await workspaceManager.switchWorkspace(workspace);
  }, []);

  const setActiveWorkspace = useCallback(async (workspaceId: string): Promise<WorkspaceInfo> => {
    return await workspaceManager.setActiveWorkspace(workspaceId);
  }, []);

  const reorderOpenedWorkspacesInSection = useCallback(async (
    section: 'assistants' | 'projects',
    sourceWorkspaceId: string,
    targetWorkspaceId: string,
    position: 'before' | 'after'
  ): Promise<void> => {
    return await workspaceManager.reorderOpenedWorkspacesInSection(
      section,
      sourceWorkspaceId,
      targetWorkspaceId,
      position
    );
  }, []);

  const scanWorkspaceInfo = useCallback(async (): Promise<WorkspaceInfo | null> => {
    return await workspaceManager.scanWorkspaceInfo();
  }, []);

  const refreshRecentWorkspaces = useCallback(async (): Promise<void> => {
    return await workspaceManager.refreshRecentWorkspaces();
  }, []);

  const activeWorkspace = isVisibleWorkspace(state.currentWorkspace)
    ? state.currentWorkspace
    : null;
  const openedWorkspacesList = useMemo(
    () => Array.from(state.openedWorkspaces.values()).filter(isVisibleWorkspace),
    [state.openedWorkspaces]
  );
  const openedWorkspaces = useMemo(
    () => new Map(openedWorkspacesList.map(workspace => [workspace.id, workspace])),
    [openedWorkspacesList]
  );
  const normalWorkspacesList = openedWorkspacesList;
  const assistantWorkspacesList = useMemo(
    () => [],
    []
  );
  const recentWorkspaces = useMemo(
    () => state.recentWorkspaces.filter(isVisibleWorkspace),
    [state.recentWorkspaces]
  );
  const hasWorkspace = !!activeWorkspace;
  const workspaceName = getWorkspaceDisplayName(activeWorkspace);
  const workspacePath = activeWorkspace?.rootPath || '';

  const contextValue: WorkspaceContextValue = {
    ...state,
    currentWorkspace: activeWorkspace,
    openedWorkspaces,
    activeWorkspaceId: activeWorkspace?.id ?? null,
    lastUsedWorkspaceId:
      activeWorkspace?.id ||
      recentWorkspaces[0]?.id ||
      openedWorkspacesList[0]?.id ||
      null,
    recentWorkspaces,
    activeWorkspace,
    openedWorkspacesList,
    normalWorkspacesList,
    assistantWorkspacesList,
    openWorkspace,
    createAssistantWorkspace,
    closeWorkspace,
    closeWorkspaceById,
    deleteAssistantWorkspace,
    resetAssistantWorkspace,
    switchWorkspace,
    setActiveWorkspace,
    reorderOpenedWorkspacesInSection,
    scanWorkspaceInfo,
    refreshRecentWorkspaces,
    hasWorkspace,
    workspaceName,
    workspacePath,
  };

  return (
    <WorkspaceContext.Provider value={contextValue}>
      {children}
    </WorkspaceContext.Provider>
  );
};

export const useWorkspaceContext = (): WorkspaceContextValue => {
  const context = useContext(WorkspaceContext);

  if (!context) {
    throw new Error('useWorkspaceContext must be used within a WorkspaceProvider');
  }

  return context;
};

export const useCurrentWorkspace = () => {
  const { activeWorkspace, loading, error, hasWorkspace, workspaceName, workspacePath } = useWorkspaceContext();

  return {
    workspace: activeWorkspace,
    loading,
    error,
    hasWorkspace,
    workspaceName,
    workspacePath,
  };
};

export const useWorkspaceEvents = (
  onWorkspaceOpened?: (workspace: WorkspaceInfo) => void,
  onWorkspaceClosed?: (workspaceId: string) => void,
  onWorkspaceSwitched?: (workspace: WorkspaceInfo) => void,
  onWorkspaceUpdated?: (workspace: WorkspaceInfo) => void
) => {
  useEffect(() => {
    const removeListener = workspaceManager.addEventListener((event: WorkspaceEvent) => {
      switch (event.type) {
        case 'workspace:opened':
          onWorkspaceOpened?.(event.workspace);
          break;
        case 'workspace:closed':
          onWorkspaceClosed?.(event.workspaceId);
          break;
        case 'workspace:switched':
          onWorkspaceSwitched?.(event.workspace);
          break;
        case 'workspace:updated':
          onWorkspaceUpdated?.(event.workspace);
          break;
      }
    });

    return removeListener;
  }, [onWorkspaceOpened, onWorkspaceClosed, onWorkspaceSwitched, onWorkspaceUpdated]);
};

export { WorkspaceContext };
