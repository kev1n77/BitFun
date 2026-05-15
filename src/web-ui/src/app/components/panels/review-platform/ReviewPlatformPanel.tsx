import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CheckCircle2,
  CircleDot,
  Clock3,
  Code2,
  GitCommitHorizontal,
  GitPullRequest,
  GitPullRequestClosed,
  KeyRound,
  Link2,
  Loader2,
  MessageSquareText,
  PanelRightOpen,
  RefreshCw,
  Search,
  ShieldCheck,
  Sparkles,
  Trash2,
  UserRound,
  XCircle,
} from 'lucide-react';
import { Button, IconButton, Input, MarkdownRenderer, Modal, Select, Tabs, TabPane, Tooltip, type SelectOption } from '@/component-library';
import { reviewPlatformAPI, systemAPI, type ReviewPlatformAccount, type ReviewPlatformPullRequest, type ReviewPlatformPullRequestDetail, type ReviewPlatformRemote, type ReviewPlatformRepositoryRef, type ReviewPlatformWorkspaceSnapshot } from '@/infrastructure/api';
import { globalEventBus } from '@/infrastructure/event-bus';
import { createLogger } from '@/shared/utils/logger';
import { notificationService } from '@/shared/notification-system';
import { useDeepReviewConsent } from '@/flow_chat/components/DeepReviewConsentDialog';
import {
  buildDeepReviewLaunchFromSessionFiles,
  buildDeepReviewPreviewFromSessionFiles,
  launchDeepReviewSession,
} from '@/flow_chat/services/DeepReviewService';
import { openBtwSessionInAuxPane, openMainSession } from '@/flow_chat/services/openBtwSession';
import { flowChatStore } from '@/flow_chat/store/FlowChatStore';
import type { FlowToolItem, Session } from '@/flow_chat/types/flow-chat';
import { findLatestCodeReviewResult, summarizeCodeReviewResult } from '@/flow_chat/utils/reviewSessionSummary';
import { deriveDeepReviewSessionConcurrencyGuard } from '@/flow_chat/utils/deepReviewCapacityGuard';
import './ReviewPlatformPanel.scss';

const log = createLogger('ReviewPlatformPanel');

interface ReviewPlatformPanelProps {
  workspacePath?: string;
}

type DetailTab = 'overview' | 'changes' | 'commits' | 'reviews';
type ListStateFilter = 'all' | 'open' | 'draft' | 'merged' | 'closed';
type SnapshotCacheState = 'none' | 'cached' | 'refreshing';

const PR_PAGE_SIZE = 10;
const CACHE_TTL_MS = 2 * 60 * 1000;
const REMOTE_STORAGE_PREFIX = 'bitfun:review-platform:last-remote:';
const MAX_LINKED_REVIEW_SESSIONS = 6;

const REVIEW_EXCLUDED_EXTENSIONS = new Set([
  '.7z', '.avi', '.avif', '.bin', '.bmp', '.class', '.dll', '.doc', '.docx', '.eot', '.exe',
  '.gif', '.gz', '.ico', '.jar', '.jpeg', '.jpg', '.lock', '.map', '.min.js', '.min.css',
  '.mov', '.mp3', '.mp4', '.otf', '.pdf', '.png', '.rar', '.so', '.svg', '.tar', '.tgz',
  '.tiff', '.ttf', '.wav', '.webm', '.webp', '.woff', '.woff2', '.xz', '.zip',
]);

const REVIEW_EXCLUDED_FILENAMES = new Set([
  'bun.lock', 'bun.lockb', 'Cargo.lock', 'composer.lock', 'Gemfile.lock', 'package-lock.json',
  'pnpm-lock.yaml', 'poetry.lock', 'Podfile.lock', 'yarn.lock',
]);

const REVIEW_EXCLUDED_PATH_SEGMENTS = new Set([
  '.cache', '.next', '.nuxt', '.output', '.parcel-cache', '.svelte-kit', '.turbo',
  'build', 'coverage', 'dist', 'node_modules', 'out', 'target',
]);

interface SnapshotCacheEntry {
  snapshot: ReviewPlatformWorkspaceSnapshot;
  fetchedAt: number;
}

interface DetailCacheEntry {
  detail: ReviewPlatformPullRequestDetail;
  fetchedAt: number;
}

interface ReviewSessionMarkerInput {
  childSessionId?: string;
  parentSessionId?: string;
  kind?: 'review' | 'deep_review';
  title?: string;
  requestedFiles?: string[];
}

interface ReviewSessionMarker {
  childSessionId: string;
  parentSessionId?: string;
  kind: 'review' | 'deep_review';
  title?: string;
  requestedFiles: string[];
}

interface LinkedReviewSession {
  childSession: Session;
  parentSession?: Session;
  marker?: ReviewSessionMarker;
  kind: 'review' | 'deep_review';
  title: string;
  requestedFiles: string[];
  issueCount: number;
  riskLevel?: string;
  lifecycle: 'running' | 'completed' | 'error' | 'idle';
  updatedAt: number;
}

const snapshotCache = new Map<string, SnapshotCacheEntry>();
const detailCache = new Map<string, DetailCacheEntry>();

function isFresh(timestamp: number): boolean {
  return Date.now() - timestamp < CACHE_TTL_MS;
}

function snapshotCacheKey(workspacePath: string, remoteId: string | null, page: number, perPage: number): string {
  return `${workspacePath}::${remoteId ?? 'default'}::${page}::${perPage}`;
}

function detailCacheKey(workspacePath: string, remoteId: string, pullRequestId: string): string {
  return `${workspacePath}::${remoteId}::${pullRequestId}`;
}

function remotePreferenceKey(workspacePath: string): string {
  return `${REMOTE_STORAGE_PREFIX}${workspacePath}`;
}

function readRememberedRemote(workspacePath?: string): string | null {
  if (!workspacePath || typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(remotePreferenceKey(workspacePath));
  } catch {
    return null;
  }
}

function rememberRemote(workspacePath: string | undefined, remoteId: string | null): void {
  if (!workspacePath || typeof window === 'undefined') return;
  try {
    const key = remotePreferenceKey(workspacePath);
    if (remoteId) {
      window.localStorage.setItem(key, remoteId);
    } else {
      window.localStorage.removeItem(key);
    }
  } catch {
    // Ignore storage failures; the selector still works for the current session.
  }
}

function formatRelativeTime(value: string): string {
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return '';
  const diffMs = Date.now() - time;
  const minutes = Math.max(1, Math.floor(diffMs / 60000));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function formatAbsoluteTime(value: string): string {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date);
}

function getPrIcon(pr: ReviewPlatformPullRequest) {
  if (pr.state === 'merged') return <GitPullRequest size={15} className="review-platform__state-icon review-platform__state-icon--merged" />;
  if (pr.state === 'closed') return <GitPullRequestClosed size={15} className="review-platform__state-icon review-platform__state-icon--closed" />;
  return <GitPullRequest size={15} className="review-platform__state-icon review-platform__state-icon--open" />;
}

function decisionLabel(decision: ReviewPlatformPullRequest['reviewDecision']): string {
  switch (decision) {
    case 'approved':
      return 'Approved';
    case 'changes_requested':
      return 'Changes requested';
    case 'commented':
      return 'Commented';
    default:
      return 'Pending review';
  }
}

function stateLabel(state: ReviewPlatformPullRequest['state']): string {
  switch (state) {
    case 'open':
      return 'Open';
    case 'draft':
      return 'Draft';
    case 'merged':
      return 'Merged';
    case 'closed':
      return 'Closed';
    default:
      return state;
  }
}

function providerLabel(remote: ReviewPlatformRemote | ReviewPlatformAccount | null): string {
  if (!remote) return 'No provider';
  switch (remote.platform) {
    case 'github':
      return 'GitHub';
    case 'gitlab':
      return 'GitLab';
    case 'codehub':
      return 'CodeHub';
    case 'gitcode':
      return 'GitCode';
    default:
      return 'Git';
  }
}

function remoteLabel(remote: ReviewPlatformRemote): string {
  return `${providerLabel(remote)} · ${remote.name} · ${remote.projectPath}`;
}

function authLabel(account: ReviewPlatformAccount | null): string {
  if (!account) return 'Disconnected';
  switch (account.authState) {
    case 'connected':
      return 'Connected';
    case 'not_required':
      return 'Public';
    case 'unsupported':
      return 'Unsupported';
    case 'expired':
      return 'Expired';
    case 'error':
      return 'Auth error';
    default:
      return 'Not connected';
  }
}

function authSourceLabel(source: ReviewPlatformAccount['authSource'] | undefined): string {
  switch (source) {
    case 'stored':
      return 'Saved token';
    case 'env':
      return 'Environment token';
    case 'unsupported':
      return 'Unsupported';
    default:
      return 'No token';
  }
}

function emptySnapshot(): ReviewPlatformWorkspaceSnapshot {
  return {
    remotes: [],
    selectedRemoteId: null,
    accounts: [],
    repository: null,
    pullRequests: [],
    pagination: {
      page: 1,
      perPage: PR_PAGE_SIZE,
      total: 0,
      hasNext: false,
    },
    capabilities: {
      canCreateReview: false,
      canReplyToThread: false,
      canResolveThread: false,
      canMerge: false,
      supportsDraftReview: false,
    },
  };
}

function diffLineClass(line: string): string {
  if (line.startsWith('+++') || line.startsWith('---')) return 'review-platform__diff-line review-platform__diff-line--meta';
  if (line.startsWith('@@')) return 'review-platform__diff-line review-platform__diff-line--hunk';
  if (line.startsWith('+')) return 'review-platform__diff-line review-platform__diff-line--add';
  if (line.startsWith('-')) return 'review-platform__diff-line review-platform__diff-line--delete';
  return 'review-platform__diff-line';
}

function fileKey(file: { path: string; oldPath?: string | null }): string {
  return `${file.oldPath ?? ''}->${file.path}`;
}

function normalizePath(value: string): string {
  return value.replace(/\\/g, '/').trim();
}

function shouldReviewFile(filePath: string): boolean {
  const normalizedPath = normalizePath(filePath);
  const segments = normalizedPath
    .split('/')
    .map(segment => segment.trim().toLowerCase())
    .filter(Boolean);

  if (segments.some(segment => REVIEW_EXCLUDED_PATH_SEGMENTS.has(segment))) {
    return false;
  }

  const fileName = normalizedPath.split('/').pop()?.trim() || normalizedPath;
  const lowerFileName = fileName.toLowerCase();

  if (REVIEW_EXCLUDED_FILENAMES.has(fileName) || REVIEW_EXCLUDED_FILENAMES.has(lowerFileName)) {
    return false;
  }

  if (lowerFileName.endsWith('.min.js') || lowerFileName.endsWith('.min.css')) {
    return false;
  }

  const extMatch = lowerFileName.match(/(\.[^.]+)$/);
  const extension = extMatch?.[1];
  return !(extension && REVIEW_EXCLUDED_EXTENSIONS.has(extension));
}

function uniquePaths(paths: string[]): string[] {
  const seen = new Set<string>();
  const next: string[] = [];
  for (const path of paths) {
    const normalized = normalizePath(path);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    next.push(normalized);
  }
  return next;
}

function pathsOverlap(left: string[], right: string[]): boolean {
  if (!left.length || !right.length) return false;
  const rightSet = new Set(right.map(normalizePath));
  return left.some(path => rightSet.has(normalizePath(path)));
}

function isReviewSessionRunning(session: Session): boolean {
  const turn = session.dialogTurns[session.dialogTurns.length - 1];
  return turn?.status === 'pending' ||
    turn?.status === 'image_analyzing' ||
    turn?.status === 'processing' ||
    turn?.status === 'finishing';
}

function reviewSessionLifecycle(session: Session): LinkedReviewSession['lifecycle'] {
  const turn = session.dialogTurns[session.dialogTurns.length - 1];
  if (session.error || turn?.status === 'error') return 'error';
  if (isReviewSessionRunning(session)) return 'running';
  if (turn?.status === 'completed') return 'completed';
  return 'idle';
}

function getSessionTitle(session?: Session, fallback = 'Review session'): string {
  return session?.title?.trim() || fallback;
}

function extractReviewSessionMarkers(session: Session): ReviewSessionMarker[] {
  const markers: ReviewSessionMarker[] = [];
  for (const turn of session.dialogTurns) {
    for (const round of turn.modelRounds) {
      for (const item of round.items) {
        if (item.type !== 'tool') continue;
        const toolItem = item as FlowToolItem;
        if (toolItem.toolName !== 'ReviewSessionSummary') continue;
        const input = (toolItem.toolCall?.input ?? {}) as ReviewSessionMarkerInput;
        if (!input.childSessionId) continue;
        markers.push({
          childSessionId: input.childSessionId,
          parentSessionId: input.parentSessionId ?? session.sessionId,
          kind: input.kind === 'deep_review' ? 'deep_review' : 'review',
          title: input.title,
          requestedFiles: uniquePaths(input.requestedFiles ?? []),
        });
      }
    }
  }
  return markers;
}

function buildPrChatPrompt(params: {
  pr: ReviewPlatformPullRequest;
  remote: ReviewPlatformRemote | null;
  repository: ReviewPlatformRepositoryRef | null;
  filePaths: string[];
  webUrl?: string;
}): string {
  const fileList = params.filePaths.length
    ? params.filePaths.map(path => `- ${path}`).join('\n')
    : '- No file list is loaded yet';
  const provider = params.remote ? providerLabel(params.remote) : 'review platform';
  const repository = params.repository?.projectPath ?? params.remote?.projectPath ?? 'current repository';

  return [
    `Review PR #${params.pr.number}: ${params.pr.title}`,
    '',
    `Provider: ${provider}`,
    `Repository: ${repository}`,
    `Branch: ${params.pr.sourceBranch} -> ${params.pr.targetBranch}`,
    params.webUrl ? `URL: ${params.webUrl}` : null,
    '',
    'Changed files:',
    fileList,
    '',
    'Please use this PR context with the current conversation. Focus on risks, review findings, and concrete fixes.',
  ].filter(Boolean).join('\n');
}

export const ReviewPlatformPanel: React.FC<ReviewPlatformPanelProps> = ({ workspacePath }) => {
  const { confirmDeepReviewLaunch, deepReviewConsentDialog } = useDeepReviewConsent();
  const [snapshot, setSnapshot] = useState<ReviewPlatformWorkspaceSnapshot>(emptySnapshot);
  const [selectedRemoteId, setSelectedRemoteId] = useState<string | null>(null);
  const [selectedPrId, setSelectedPrId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ReviewPlatformPullRequestDetail | null>(null);
  const [flowState, setFlowState] = useState(() => flowChatStore.getState());
  const [activeTab, setActiveTab] = useState<DetailTab>('overview');
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [launchingDeepReview, setLaunchingDeepReview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [stateFilter, setStateFilter] = useState<ListStateFilter>('all');
  const [pageIndex, setPageIndex] = useState(0);
  const [expandedFileKeys, setExpandedFileKeys] = useState<Set<string>>(() => new Set());
  const [snapshotCacheState, setSnapshotCacheState] = useState<SnapshotCacheState>('none');
  const [authModalOpen, setAuthModalOpen] = useState(false);
  const [authToken, setAuthToken] = useState('');
  const [authSaving, setAuthSaving] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);

  const repository = snapshot.repository;
  const account = snapshot.accounts[0] ?? null;
  const selectedRemote = useMemo(
    () => snapshot.remotes.find(remote => remote.id === selectedRemoteId) ?? snapshot.remotes[0] ?? null,
    [selectedRemoteId, snapshot.remotes],
  );
  const selectedPr = useMemo(
    () => snapshot.pullRequests.find(pr => pr.id === selectedPrId) ?? null,
    [selectedPrId, snapshot.pullRequests],
  );
  const prFilePaths = useMemo(
    () => uniquePaths((detail?.files ?? []).map(file => file.path)),
    [detail?.files],
  );
  const reviewablePrFilePaths = useMemo(
    () => prFilePaths.filter(shouldReviewFile),
    [prFilePaths],
  );
  const skippedReviewFileCount = Math.max(0, prFilePaths.length - reviewablePrFilePaths.length);
  const remoteOptions = useMemo<SelectOption[]>(
    () => snapshot.remotes.map(remote => ({
      value: remote.id,
      label: remoteLabel(remote),
      description: `${remote.host} · ${authLabel(account && account.id === remote.id ? account : null)}`,
    })),
    [account, snapshot.remotes],
  );

  const loadSnapshot = useCallback(async (nextRemoteId?: string | null, options?: { force?: boolean; page?: number }) => {
    if (!workspacePath) {
      setSnapshot(emptySnapshot());
      setSelectedRemoteId(null);
      setSelectedPrId(null);
      setDetail(null);
      setError('No active workspace is available.');
      setLoading(false);
      return;
    }

    const requestedRemoteId = nextRemoteId !== undefined ? nextRemoteId : readRememberedRemote(workspacePath);
    const requestedPage = Math.max(1, options?.page ?? 1);
    const requestedCacheKey = snapshotCacheKey(workspacePath, requestedRemoteId ?? null, requestedPage, PR_PAGE_SIZE);
    const cached = snapshotCache.get(requestedCacheKey);
    const force = options?.force === true;

    if (cached && !force) {
      const remoteId = cached.snapshot.selectedRemoteId ?? cached.snapshot.remotes[0]?.id ?? null;
      setSnapshot(cached.snapshot);
      setSelectedRemoteId(remoteId);
      setPageIndex(Math.max(0, (cached.snapshot.pagination.page || requestedPage) - 1));
      setSelectedPrId(null);
      setDetail(null);
      setError(null);
      setSnapshotCacheState(isFresh(cached.fetchedAt) ? 'cached' : 'refreshing');
      if (isFresh(cached.fetchedAt)) {
        setLoading(false);
        return;
      }
    } else {
      setSnapshot(emptySnapshot());
      setSelectedPrId(null);
      setDetail(null);
      setSnapshotCacheState('none');
    }

    setLoading(true);
    setError(null);
    try {
      const next = await reviewPlatformAPI.getWorkspaceSnapshot(workspacePath, requestedRemoteId ?? null, requestedPage, PR_PAGE_SIZE);
      setSnapshot(next);
      const remoteId = next.selectedRemoteId ?? next.remotes[0]?.id ?? null;
      setSelectedRemoteId(remoteId);
      setPageIndex(Math.max(0, (next.pagination.page || requestedPage) - 1));
      rememberRemote(workspacePath, remoteId);
      setSelectedPrId(null);
      setDetail(null);
      const entry = { snapshot: next, fetchedAt: Date.now() };
      snapshotCache.set(requestedCacheKey, entry);
      if (remoteId) {
        snapshotCache.set(snapshotCacheKey(workspacePath, remoteId, requestedPage, PR_PAGE_SIZE), entry);
      }
      setSnapshotCacheState('cached');
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load pull requests';
      setError(message);
      if (!cached) {
        setSnapshot(emptySnapshot());
      }
      log.error('Failed to load review platform snapshot', { workspacePath, error: err });
    } finally {
      setLoading(false);
    }
  }, [workspacePath]);

  const loadDetail = useCallback(async (repo: ReviewPlatformRepositoryRef, remoteId: string, pullRequestId: string) => {
    const repositoryPath = workspacePath || repo.workspacePath || '';
    const cacheKey = detailCacheKey(repositoryPath, remoteId, pullRequestId);
    const cached = detailCache.get(cacheKey);

    if (cached) {
      setDetail(cached.detail);
      if (isFresh(cached.fetchedAt)) {
        setDetailLoading(false);
        return;
      }
    } else {
      setDetail(null);
    }

    setDetailLoading(true);
    try {
      const nextDetail = await reviewPlatformAPI.getPullRequestDetail(repositoryPath, remoteId, pullRequestId);
      setDetail(nextDetail);
      detailCache.set(cacheKey, { detail: nextDetail, fetchedAt: Date.now() });
    } catch (err) {
      log.error('Failed to load pull request detail', { pullRequestId, error: err });
      if (!cached) {
        setDetail(null);
      }
    } finally {
      setDetailLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  useEffect(() => flowChatStore.subscribe(setFlowState), []);

  useEffect(() => {
    if (!selectedRemoteId) {
      setDetail(null);
      return;
    }
    if (!repository || !selectedPrId) {
      setDetail(null);
      return;
    }
    void loadDetail(repository, selectedRemoteId, selectedPrId);
  }, [loadDetail, repository, selectedPrId, selectedRemoteId]);

  useEffect(() => {
    if (!snapshot.remotes.length) return;
    if (!selectedRemoteId && snapshot.selectedRemoteId) {
      setSelectedRemoteId(snapshot.selectedRemoteId);
    }
  }, [selectedRemoteId, snapshot.remotes.length, snapshot.selectedRemoteId]);

  useEffect(() => {
    if (!detail) {
      setExpandedFileKeys(new Set());
      return;
    }
    setExpandedFileKeys(new Set(detail.files.slice(0, 1).map(fileKey)));
  }, [detail]);

  const visiblePullRequests = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return snapshot.pullRequests.filter(pr => {
      if (stateFilter !== 'all' && pr.state !== stateFilter) return false;
      if (!needle) return true;
      return [
        pr.title,
        pr.author,
        pr.sourceBranch,
        pr.targetBranch,
        `#${pr.number}`,
      ].some(value => value.toLowerCase().includes(needle));
    });
  }, [query, snapshot.pullRequests, stateFilter]);

  const parentSession = useMemo(() => {
    const sessions = Array.from(flowState.sessions.values());
    const activeSession = flowState.activeSessionId
      ? flowState.sessions.get(flowState.activeSessionId)
      : undefined;
    const sameWorkspace = (session?: Session) =>
      Boolean(session && (!workspacePath || normalizePath(session.workspacePath ?? '') === normalizePath(workspacePath)));

    if (activeSession?.sessionKind === 'normal' && sameWorkspace(activeSession)) {
      return activeSession;
    }

    if (
      activeSession &&
      (activeSession.sessionKind === 'review' || activeSession.sessionKind === 'deep_review') &&
      activeSession.parentSessionId
    ) {
      const parent = flowState.sessions.get(activeSession.parentSessionId);
      if (parent?.sessionKind === 'normal' && sameWorkspace(parent)) {
        return parent;
      }
    }

    return sessions
      .filter(session => session.sessionKind === 'normal' && sameWorkspace(session))
      .sort((left, right) => (right.lastActiveAt || right.updatedAt || right.createdAt) - (left.lastActiveAt || left.updatedAt || left.createdAt))[0];
  }, [flowState.activeSessionId, flowState.sessions, workspacePath]);

  const linkedReviewSessions = useMemo<LinkedReviewSession[]>(() => {
    const sessions = Array.from(flowState.sessions.values());
    const markersByChildId = new Map<string, ReviewSessionMarker>();
    for (const session of sessions) {
      for (const marker of extractReviewSessionMarkers(session)) {
        markersByChildId.set(marker.childSessionId, marker);
      }
    }

    const selectedPaths = prFilePaths;
    const sameWorkspace = (session: Session) =>
      !workspacePath || normalizePath(session.workspacePath ?? '') === normalizePath(workspacePath);

    return sessions
      .filter(session =>
        (session.sessionKind === 'review' || session.sessionKind === 'deep_review') &&
        sameWorkspace(session),
      )
      .map((session): LinkedReviewSession | null => {
        const marker = markersByChildId.get(session.sessionId);
        const requestedFiles = marker?.requestedFiles ?? [];
        if (selectedPaths.length > 0 && requestedFiles.length > 0 && !pathsOverlap(selectedPaths, requestedFiles)) {
          return null;
        }

        const reviewResult = findLatestCodeReviewResult(session);
        const summary = summarizeCodeReviewResult(reviewResult);
        const kind = session.sessionKind === 'deep_review' ? 'deep_review' : 'review';
        return {
          childSession: session,
          parentSession: marker?.parentSessionId ? flowState.sessions.get(marker.parentSessionId) : undefined,
          marker,
          kind,
          title: marker?.title || getSessionTitle(session, kind === 'deep_review' ? 'Deep review' : 'Code review'),
          requestedFiles,
          issueCount: summary.issueCount,
          riskLevel: summary.riskLevel,
          lifecycle: reviewSessionLifecycle(session),
          updatedAt: session.lastActiveAt || session.updatedAt || session.createdAt,
        };
      })
      .filter((session): session is LinkedReviewSession => Boolean(session))
      .sort((left, right) => right.updatedAt - left.updatedAt)
      .slice(0, MAX_LINKED_REVIEW_SESSIONS);
  }, [flowState.sessions, prFilePaths, workspacePath]);

  const pagination = snapshot.pagination;
  const totalCount = pagination.total ?? null;
  const currentPageIndex = Math.max(0, (pagination.page || pageIndex + 1) - 1);
  const totalPages = totalCount !== null
    ? Math.max(1, Math.ceil(totalCount / pagination.perPage))
    : currentPageIndex + (pagination.hasNext ? 2 : 1);
  const pageStart = snapshot.pullRequests.length ? currentPageIndex * pagination.perPage + 1 : 0;
  const pageEnd = totalCount !== null
    ? Math.min(totalCount, currentPageIndex * pagination.perPage + snapshot.pullRequests.length)
    : currentPageIndex * pagination.perPage + snapshot.pullRequests.length;

  const summary = useMemo(() => {
    const prs = snapshot.pullRequests;
    return {
      open: prs.filter(pr => pr.state === 'open').length,
      draft: prs.filter(pr => pr.state === 'draft').length,
      merged: prs.filter(pr => pr.state === 'merged').length,
      reviewRequired: prs.filter(pr => pr.reviewDecision === 'changes_requested' || pr.reviewDecision === 'pending').length,
    };
  }, [snapshot.pullRequests]);

  const headerLabel = selectedRemote ? remoteLabel(selectedRemote) : repository ? repository.projectPath : 'No repository';

  const handleRemoteChange = useCallback((value: string | number | (string | number)[]) => {
    const remoteId = Array.isArray(value) ? String(value[0] ?? '') : String(value);
    setSelectedRemoteId(remoteId || null);
    setSelectedPrId(null);
    setDetail(null);
    setPageIndex(0);
    rememberRemote(workspacePath, remoteId || null);
    void loadSnapshot(remoteId || null, { page: 1 });
  }, [loadSnapshot, workspacePath]);

  const handlePageChange = useCallback((nextPageIndex: number) => {
    const nextPage = Math.max(1, nextPageIndex + 1);
    setSelectedPrId(null);
    setDetail(null);
    setPageIndex(nextPage - 1);
    void loadSnapshot(selectedRemoteId, { page: nextPage });
  }, [loadSnapshot, selectedRemoteId]);

  const toggleFileExpanded = useCallback((key: string) => {
    setExpandedFileKeys(prev => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

  const handleOpenExternal = useCallback(async () => {
    const webUrl = selectedPr?.webUrl;
    if (!webUrl) return;
    try {
      await systemAPI.openExternal(webUrl);
    } catch (error) {
      log.error('Failed to open pull request URL', { error, webUrl });
    }
  }, [selectedPr?.webUrl]);

  const handleOpenParentChat = useCallback(async () => {
    if (!parentSession) {
      notificationService.warning('Open or create a chat session before linking PR context.', { duration: 3500 });
      return;
    }
    await openMainSession(parentSession.sessionId);
  }, [parentSession]);

  const handleFillPrContext = useCallback(async () => {
    if (!selectedPr) return;
    if (!parentSession) {
      notificationService.warning('Open or create a chat session before sending PR context.', { duration: 3500 });
      return;
    }

    await openMainSession(parentSession.sessionId);
    globalEventBus.emit('fill-chat-input', {
      content: buildPrChatPrompt({
        pr: selectedPr,
        remote: selectedRemote,
        repository,
        filePaths: prFilePaths,
        webUrl: selectedPr.webUrl,
      }),
      mode: 'replace',
    });
  }, [parentSession, prFilePaths, repository, selectedPr, selectedRemote]);

  const handleOpenLinkedReview = useCallback(async (linked: LinkedReviewSession) => {
    const parentId = linked.parentSession?.sessionId ?? linked.childSession.parentSessionId ?? parentSession?.sessionId;
    if (!parentId) {
      notificationService.warning('The parent chat session for this review is unavailable.', { duration: 3500 });
      return;
    }

    await openMainSession(parentId);
    openBtwSessionInAuxPane({
      childSessionId: linked.childSession.sessionId,
      parentSessionId: parentId,
      workspacePath: linked.childSession.workspacePath,
      expand: true,
    });
  }, [parentSession?.sessionId]);

  const handleRunDeepReview = useCallback(async () => {
    if (!selectedPr) return;
    if (!parentSession) {
      notificationService.warning('Open or create a chat session before launching Deep Review.', { duration: 3500 });
      return;
    }
    if (!workspacePath) {
      notificationService.warning('No active workspace is available for Deep Review.', { duration: 3500 });
      return;
    }
    if (!reviewablePrFilePaths.length) {
      notificationService.warning('No reviewable files are loaded for this pull request.', { duration: 3500 });
      return;
    }

    setLaunchingDeepReview(true);
    try {
      const extraContext = [
        `Pull request: #${selectedPr.number} ${selectedPr.title}`,
        `Branch: ${selectedPr.sourceBranch} -> ${selectedPr.targetBranch}`,
        selectedPr.webUrl ? `URL: ${selectedPr.webUrl}` : null,
        selectedRemote ? `Provider: ${providerLabel(selectedRemote)} ${selectedRemote.projectPath}` : null,
      ].filter(Boolean).join('\n');
      const preview = await buildDeepReviewPreviewFromSessionFiles(reviewablePrFilePaths, workspacePath);
      const confirmed = await confirmDeepReviewLaunch(preview, {
        sessionConcurrencyGuard: deriveDeepReviewSessionConcurrencyGuard(flowChatStore.getState(), parentSession.sessionId),
      });
      if (!confirmed) {
        return;
      }

      if (skippedReviewFileCount > 0) {
        notificationService.info(
          `Deep Review will analyze ${reviewablePrFilePaths.length} files and skip ${skippedReviewFileCount} generated, binary, or lock files.`,
          { duration: 4000 },
        );
      }

      const { prompt, runManifest } = await buildDeepReviewLaunchFromSessionFiles(
        reviewablePrFilePaths,
        extraContext,
        workspacePath,
      );
      const fileList = reviewablePrFilePaths.map(path => `- ${path}`).join('\n');
      await launchDeepReviewSession({
        parentSessionId: parentSession.sessionId,
        workspacePath,
        prompt,
        displayMessage: `Deep review PR #${selectedPr.number}: ${selectedPr.title}\n\n${fileList}`,
        runManifest,
        childSessionName: `Deep review PR #${selectedPr.number}`,
        requestedFiles: reviewablePrFilePaths,
      });
      notificationService.success('Deep Review started for this pull request.', { duration: 3000 });
    } catch (err) {
      log.error('Failed to launch PR Deep Review', {
        pullRequestId: selectedPr.id,
        parentSessionId: parentSession.sessionId,
        error: err,
      });
      notificationService.error(err instanceof Error ? err.message : 'Failed to launch Deep Review.', { duration: 5000 });
    } finally {
      setLaunchingDeepReview(false);
    }
  }, [
    confirmDeepReviewLaunch,
    parentSession,
    reviewablePrFilePaths,
    selectedPr,
    selectedRemote,
    skippedReviewFileCount,
    workspacePath,
  ]);

  const refreshAuthSnapshot = useCallback((remoteId: string | null) => {
    snapshotCache.clear();
    detailCache.clear();
    void loadSnapshot(remoteId, { force: true, page: currentPageIndex + 1 });
  }, [currentPageIndex, loadSnapshot]);

  const handleOpenAuthModal = useCallback(() => {
    setAuthToken('');
    setAuthError(null);
    setAuthModalOpen(true);
  }, []);

  const handleSaveAuthToken = useCallback(async () => {
    if (!selectedRemote || selectedRemote.platform === 'unknown') return;
    const token = authToken.trim();
    if (!token) {
      setAuthError('Token is required.');
      return;
    }

    setAuthSaving(true);
    setAuthError(null);
    try {
      await reviewPlatformAPI.updateAuthToken({
        platform: selectedRemote.platform,
        host: selectedRemote.host,
        token,
      });
      setAuthModalOpen(false);
      setAuthToken('');
      refreshAuthSnapshot(selectedRemote.id);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to save token.';
      setAuthError(message);
      log.error('Failed to save review platform token', { error: err, host: selectedRemote.host });
    } finally {
      setAuthSaving(false);
    }
  }, [authToken, refreshAuthSnapshot, selectedRemote]);

  const handleClearAuthToken = useCallback(async () => {
    if (!selectedRemote || selectedRemote.platform === 'unknown') return;
    setAuthSaving(true);
    setAuthError(null);
    try {
      await reviewPlatformAPI.clearAuthToken({
        platform: selectedRemote.platform,
        host: selectedRemote.host,
      });
      refreshAuthSnapshot(selectedRemote.id);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to clear token.';
      setAuthError(message);
      setAuthModalOpen(true);
      log.error('Failed to clear review platform token', { error: err, host: selectedRemote.host });
    } finally {
      setAuthSaving(false);
    }
  }, [refreshAuthSnapshot, selectedRemote]);

  const remoteStatus = selectedRemote
    ? `${providerLabel(selectedRemote)} · ${authLabel(account)}`
    : 'No remote detected';
  const displayPr = detail ?? selectedPr;
  const checksText = displayPr && displayPr.checks.total > 0
    ? `${displayPr.checks.passed}/${displayPr.checks.total}`
    : 'N/A';
  const emptyStateMessage = account?.message
    || selectedRemote?.message
    || (snapshot.remotes.length ? 'No pull requests match the current filter.' : 'No supported remotes were detected.');
  const loadingLabel = loading
    ? snapshotCacheState === 'refreshing'
      ? 'Refreshing cached pull requests...'
      : 'Loading pull requests...'
    : snapshotCacheState === 'cached'
      ? 'Cached pull requests'
      : null;
  const parentSessionLabel = parentSession ? getSessionTitle(parentSession, 'Current chat') : 'No chat session linked';

  return (
    <div className="review-platform">
      <div className="review-platform__topbar">
        <div className="review-platform__brand">
          <span className="review-platform__brand-icon"><GitPullRequest size={17} /></span>
          <div className="review-platform__brand-copy">
            <span className="review-platform__title">Pull Requests</span>
            <span className="review-platform__subtitle">{headerLabel}</span>
          </div>
        </div>

        <div className="review-platform__topbar-actions">
          <div className="review-platform__remote-select">
            <Select
              size="small"
              value={selectedRemoteId ?? ''}
              options={remoteOptions}
              placeholder="Select remote"
              disabled={!remoteOptions.length || loading}
              searchable
              onChange={handleRemoteChange}
            />
          </div>
          {account && (
            <Tooltip content={`${account.label} · ${authSourceLabel(account.authSource)}`}>
              <span className={`review-platform__account review-platform__account--${account.authState}`}>
                <ShieldCheck size={13} />
                <span>{authLabel(account)}</span>
              </span>
            </Tooltip>
          )}
          <IconButton
            className="review-platform__icon-button"
            size="xs"
            variant="ghost"
            tooltip={account?.authSource === 'stored' ? 'Update token' : 'Add token'}
            disabled={!selectedRemote || selectedRemote.platform === 'unknown' || loading || authSaving}
            onClick={handleOpenAuthModal}
          >
            <KeyRound size={14} />
          </IconButton>
          {account?.authSource === 'stored' && (
            <IconButton
              className="review-platform__icon-button"
              size="xs"
              variant="ghost"
              tooltip="Clear token"
              disabled={!selectedRemote || loading || authSaving}
              onClick={handleClearAuthToken}
            >
              <Trash2 size={14} />
            </IconButton>
          )}
          <IconButton
            className="review-platform__icon-button"
            size="xs"
            variant="ghost"
            tooltip="Refresh"
            onClick={() => void loadSnapshot(selectedRemoteId, { force: true, page: currentPageIndex + 1 })}
            isLoading={loading}
          >
            <RefreshCw size={14} />
          </IconButton>
        </div>
      </div>

      <div className="review-platform__subbar">
        <div className="review-platform__status-line">
          <span><CircleDot size={12} /> {summary.open} open</span>
          <span><GitPullRequestClosed size={12} /> {summary.merged} merged</span>
          <span><Sparkles size={12} /> {summary.reviewRequired} review</span>
          <span><Link2 size={12} /> {remoteStatus}</span>
          {loadingLabel && (
            <span className="review-platform__loading-inline">
              {loading && <Loader2 size={12} />}
              {loadingLabel}
            </span>
          )}
        </div>
      </div>

      <div className="review-platform__body">
        <aside className="review-platform__list" aria-label="Pull request list">
          <div className="review-platform__list-toolbar">
            <Input
              inputSize="small"
              value={query}
              onChange={event => setQuery(event.target.value)}
              placeholder="Search pull requests"
              prefix={<Search size={14} />}
              suffix={query ? <IconButton className="review-platform__icon-button" size="xs" variant="ghost" onClick={() => setQuery('')}><XCircle size={14} /></IconButton> : undefined}
            />
            <div className="review-platform__state-filters">
              {(['all', 'open', 'draft', 'merged', 'closed'] as ListStateFilter[]).map(state => (
                <button
                  key={state}
                  type="button"
                  className={`review-platform__state-chip${stateFilter === state ? ' is-active' : ''}`}
                  onClick={() => setStateFilter(state)}
                >
                  {state === 'all' ? 'All' : stateLabel(state)}
                </button>
              ))}
            </div>
          </div>

          <div className="review-platform__list-scroll">
            {loading && (
              <div className="review-platform__empty-state">Loading pull requests...</div>
            )}
            {error && (
              <div className="review-platform__error-state">
                <XCircle size={16} />
                <span>{error}</span>
                <Button className="review-platform__panel-button" size="small" variant="secondary" onClick={() => void loadSnapshot(selectedRemoteId, { force: true, page: currentPageIndex + 1 })}>
                  Retry
                </Button>
              </div>
            )}
            {!loading && !error && !visiblePullRequests.length && (
              <div className="review-platform__empty-state">
                <GitPullRequest size={18} />
                <span>{emptyStateMessage}</span>
              </div>
            )}
            {!loading && !error && visiblePullRequests.map(pr => (
              (() => {
                return (
                  <button
                    key={pr.id}
                    type="button"
                    className={`review-platform__pr-row${selectedPrId === pr.id ? ' is-selected' : ''}`}
                    onClick={() => setSelectedPrId(pr.id)}
                  >
                    <span className="review-platform__pr-icon">{getPrIcon(pr)}</span>
                    <span className="review-platform__pr-main">
                      <span className="review-platform__pr-title">{pr.title}</span>
                      <span className="review-platform__pr-meta">
                        #{pr.number} · {pr.sourceBranch} → {pr.targetBranch}
                      </span>
                      <span className="review-platform__pr-meta review-platform__pr-meta--secondary">
                        {pr.author} · {formatRelativeTime(pr.updatedAt)}
                      </span>
                    </span>
                    <span className="review-platform__pr-stats">
                      <span className={`review-platform__decision review-platform__decision--${pr.reviewDecision}`}>
                        {decisionLabel(pr.reviewDecision)}
                      </span>
                      <span className="review-platform__counts">
                        <span>{pr.changedFiles} files</span>
                        <span className="review-platform__additions">+{pr.additions}</span>
                        <span className="review-platform__deletions">-{pr.deletions}</span>
                      </span>
                    </span>
                  </button>
                );
              })()
            ))}
          </div>
          {!loading && !error && (totalPages > 1 || pagination.hasNext) && (
            <div className="review-platform__pagination">
              <IconButton
                className="review-platform__icon-button"
                size="xs"
                variant="ghost"
                tooltip="Previous page"
                disabled={currentPageIndex === 0}
                onClick={() => handlePageChange(currentPageIndex - 1)}
              >
                <ChevronLeft size={14} />
              </IconButton>
              <span>
                {pageStart}-{pageEnd} of {totalCount ?? `${pageEnd}+`}
              </span>
              <IconButton
                className="review-platform__icon-button"
                size="xs"
                variant="ghost"
                tooltip="Next page"
                disabled={!pagination.hasNext && currentPageIndex >= totalPages - 1}
                onClick={() => handlePageChange(currentPageIndex + 1)}
              >
                <ChevronRight size={14} />
              </IconButton>
            </div>
          )}
        </aside>

        <main className="review-platform__detail">
          {!selectedPr && !loading && (
            <div className="review-platform__detail-empty">
              <GitPullRequest size={24} />
              <span>Select a pull request to inspect it.</span>
            </div>
          )}

          {selectedPr && (
            <>
              <div className="review-platform__detail-header">
                <div className="review-platform__detail-title-block">
                  <div className="review-platform__detail-title-row">
                    {getPrIcon(selectedPr)}
                    <h3>{selectedPr.title}</h3>
                  </div>
                  <div className="review-platform__detail-meta">
                    <span>#{selectedPr.number}</span>
                    <span><UserRound size={12} /> {selectedPr.author}</span>
                    <span><Clock3 size={12} /> {formatAbsoluteTime(selectedPr.updatedAt) || formatRelativeTime(selectedPr.updatedAt)}</span>
                    <span><Code2 size={12} /> {selectedPr.sourceBranch} → {selectedPr.targetBranch}</span>
                  </div>
                </div>
                <div className="review-platform__detail-actions">
                  <Button className="review-platform__panel-button" size="small" variant="secondary" onClick={handleRunDeepReview} disabled={!selectedPr || detailLoading || !reviewablePrFilePaths.length || launchingDeepReview} isLoading={launchingDeepReview}>
                    <Sparkles size={13} />
                    Deep Review
                  </Button>
                  <Button className="review-platform__panel-button" size="small" variant="secondary" onClick={handleFillPrContext} disabled={!selectedPr}>
                    <MessageSquareText size={13} />
                    Ask
                  </Button>
                  <Button className="review-platform__panel-button" size="small" variant="secondary" onClick={handleOpenExternal} disabled={!selectedPr.webUrl}>
                    <Link2 size={13} />
                    Open
                  </Button>
                </div>
              </div>

              <div className="review-platform__summary-strip">
                <div className="review-platform__summary-card">
                  <span className="review-platform__summary-label">Files</span>
                  <strong>{displayPr?.changedFiles ?? selectedPr.changedFiles}</strong>
                </div>
                <div className="review-platform__summary-card">
                  <span className="review-platform__summary-label">Additions</span>
                  <strong className="review-platform__additions">+{displayPr?.additions ?? selectedPr.additions}</strong>
                </div>
                <div className="review-platform__summary-card">
                  <span className="review-platform__summary-label">Deletions</span>
                  <strong className="review-platform__deletions">-{displayPr?.deletions ?? selectedPr.deletions}</strong>
                </div>
                <div className="review-platform__summary-card">
                  <span className="review-platform__summary-label">Checks</span>
                  <strong>{checksText}</strong>
                </div>
              </div>

              <section className="review-platform__agent-link-panel" aria-label="Conversation and review agents">
                <div className="review-platform__agent-link-main">
                  <span className="review-platform__agent-link-label">Conversation link</span>
                  <strong>{parentSessionLabel}</strong>
                  <span>
                    {reviewablePrFilePaths.length} reviewable files
                    {skippedReviewFileCount > 0 ? ` · ${skippedReviewFileCount} skipped` : ''}
                    {linkedReviewSessions.length > 0 ? ` · ${linkedReviewSessions.length} related review sessions` : ''}
                  </span>
                </div>
                <div className="review-platform__agent-link-actions">
                  <Button className="review-platform__panel-button" size="small" variant="ghost" onClick={handleOpenParentChat} disabled={!parentSession}>
                    <MessageSquareText size={13} />
                    Open chat
                  </Button>
                  <Button className="review-platform__panel-button" size="small" variant="ghost" onClick={handleFillPrContext} disabled={!parentSession || !selectedPr}>
                    <Code2 size={13} />
                    Insert context
                  </Button>
                </div>
              </section>

              <Tabs
                activeKey={activeTab}
                onChange={(key) => setActiveTab(key as DetailTab)}
                type="pill"
                size="small"
                className="review-platform__tabs"
              >
                <TabPane tabKey="overview" label="Overview">
                  <section className="review-platform__tab-content">
                    <div className="review-platform__review-sessions">
                      <div className="review-platform__review-sessions-head">
                        <span>Review activity</span>
                        <span>{linkedReviewSessions.length ? 'Synced from chat sessions' : 'No related review session yet'}</span>
                      </div>
                      {linkedReviewSessions.length > 0 ? (
                        <div className="review-platform__review-session-list">
                          {linkedReviewSessions.map((linked) => (
                            <button
                              key={linked.childSession.sessionId}
                              type="button"
                              className={`review-platform__review-session review-platform__review-session--${linked.lifecycle}`}
                              onClick={() => void handleOpenLinkedReview(linked)}
                            >
                              <span className="review-platform__review-session-icon">
                                {linked.kind === 'deep_review' ? <Sparkles size={14} /> : <ShieldCheck size={14} />}
                              </span>
                              <span className="review-platform__review-session-main">
                                <strong>{linked.title}</strong>
                                <span>
                                  {linked.lifecycle}
                                  {linked.issueCount > 0 ? ` · ${linked.issueCount} issues` : ' · no issues reported'}
                                  {linked.riskLevel ? ` · ${linked.riskLevel} risk` : ''}
                                </span>
                              </span>
                              <PanelRightOpen size={14} />
                            </button>
                          ))}
                        </div>
                      ) : (
                        <div className="review-platform__review-session-empty">
                          Start Deep Review to attach reviewer output and remediation actions to this PR.
                        </div>
                      )}
                    </div>
                    <div className="review-platform__body-markdown">
                      {detail?.body
                        ? <MarkdownRenderer content={detail.body} basePath={workspacePath} />
                        : <span className="review-platform__loading">Loading pull request summary...</span>}
                    </div>
                    <div className="review-platform__capability-row">
                      <span className="review-platform__capability-chip">State: {stateLabel(displayPr?.state ?? selectedPr.state)}</span>
                      <span className={`review-platform__decision review-platform__decision--${displayPr?.reviewDecision ?? selectedPr.reviewDecision}`}>
                        {decisionLabel(displayPr?.reviewDecision ?? selectedPr.reviewDecision)}
                      </span>
                      <span className="review-platform__capability-chip">
                        {selectedRemote ? providerLabel(selectedRemote) : 'Unknown provider'}
                      </span>
                    </div>
                    <div className="review-platform__summary-grid">
                      <span><CheckCircle2 size={13} /> {checksText} checks</span>
                      <span><MessageSquareText size={13} /> {displayPr?.comments ?? selectedPr.comments} comments</span>
                      <span><UserRound size={13} /> {displayPr?.author ?? selectedPr.author}</span>
                      <span><Clock3 size={13} /> {formatAbsoluteTime(displayPr?.updatedAt ?? selectedPr.updatedAt)}</span>
                    </div>
                  </section>
                </TabPane>

                <TabPane tabKey="changes" label="Changes">
                  <section className="review-platform__tab-content review-platform__file-list">
                    {detailLoading && <span className="review-platform__loading">Loading files...</span>}
                    {detail?.files.map(file => {
                      const key = fileKey(file);
                      const isExpanded = expandedFileKeys.has(key);
                      return (
                        <article key={key} className="review-platform__file-card">
                          <button
                            type="button"
                            className="review-platform__file-row"
                            aria-expanded={isExpanded}
                            onClick={() => toggleFileExpanded(key)}
                          >
                            <span className="review-platform__file-toggle">
                              {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                            </span>
                            <span className={`review-platform__file-status review-platform__file-status--${file.status}`}>
                              {file.status}
                            </span>
                            <span className="review-platform__file-path">{file.path}</span>
                            <span className="review-platform__file-delta">
                              <span className="review-platform__additions">+{file.additions}</span>
                              <span className="review-platform__deletions">-{file.deletions}</span>
                            </span>
                          </button>
                          {isExpanded && (
                            file.patch ? (
                              <pre className="review-platform__diff-block" aria-label={`Diff for ${file.path}`}>
                                {file.patch.split('\n').map((line, index) => (
                                  <span key={`${file.path}-${index}`} className={diffLineClass(line)}>
                                    {line || ' '}
                                  </span>
                                ))}
                              </pre>
                            ) : (
                              <div className="review-platform__diff-empty">No inline diff is available for this file.</div>
                            )
                          )}
                        </article>
                      );
                    })}
                    {!detailLoading && detail && detail.files.length === 0 && (
                      <div className="review-platform__empty-state">No changed files were returned by this provider.</div>
                    )}
                  </section>
                </TabPane>

                <TabPane tabKey="commits" label="Commits">
                  <section className="review-platform__tab-content review-platform__timeline">
                    {detail?.commits.map(commit => (
                      <div key={commit.hash} className="review-platform__timeline-item">
                        <GitCommitHorizontal size={14} />
                        <span className="review-platform__timeline-main">
                          <strong>{commit.title}</strong>
                          <span>{commit.author} · {formatRelativeTime(commit.committedAt)}</span>
                        </span>
                        <code>{commit.shortHash}</code>
                      </div>
                    ))}
                  </section>
                </TabPane>

                <TabPane tabKey="reviews" label="Reviews">
                  <section className="review-platform__tab-content review-platform__threads">
                    {detail?.threads.map(thread => (
                      <article key={thread.id} className={`review-platform__thread${thread.resolved ? ' is-resolved' : ''}`}>
                        <div className="review-platform__thread-head">
                          <span>{thread.author}</span>
                          <span>{thread.resolved ? 'Resolved' : 'Open'}</span>
                        </div>
                        <p>{thread.body}</p>
                        {thread.filePath && (
                          <span className="review-platform__thread-anchor">
                            {thread.filePath}{thread.line ? `:${thread.line}` : ''}
                          </span>
                        )}
                      </article>
                    ))}
                  </section>
                </TabPane>
              </Tabs>
            </>
          )}
        </main>
      </div>
      <Modal
        isOpen={authModalOpen}
        onClose={() => {
          if (!authSaving) {
            setAuthModalOpen(false);
            setAuthError(null);
          }
        }}
        title={`${selectedRemote ? providerLabel(selectedRemote) : 'Provider'} token`}
        size="small"
        contentInset
      >
        <form
          className="review-platform__auth-form"
          onSubmit={(event) => {
            event.preventDefault();
            void handleSaveAuthToken();
          }}
        >
          <div className="review-platform__auth-target">
            <span>{selectedRemote?.host ?? 'No remote'}</span>
            <strong>{selectedRemote?.projectPath ?? ''}</strong>
          </div>
          <Input
            type="password"
            autoComplete="off"
            autoFocus
            label="Token"
            value={authToken}
            disabled={authSaving}
            error={Boolean(authError)}
            errorMessage={authError ?? undefined}
            onChange={event => {
              setAuthToken(event.target.value);
              if (authError) setAuthError(null);
            }}
          />
          <div className="review-platform__auth-actions">
            <Button
              type="button"
              className="review-platform__panel-button"
              size="small"
              variant="ghost"
              disabled={authSaving}
              onClick={() => {
                setAuthModalOpen(false);
                setAuthError(null);
              }}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              className="review-platform__panel-button"
              size="small"
              variant="primary"
              isLoading={authSaving}
              disabled={!authToken.trim()}
            >
              Save
            </Button>
          </div>
        </form>
      </Modal>
      {deepReviewConsentDialog}
    </div>
  );
};

export default ReviewPlatformPanel;
