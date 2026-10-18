import { Dialog } from '@openbitfun/ui';
import { useId, useSyncExternalStore } from 'react';
import { useCurrentWorkspace } from '@/infrastructure/contexts/WorkspaceContext';
import { getActiveSurfaceScope, onSurfaceActivated } from '@/infrastructure/peer-device/deviceSurface';
import CreateAgentPage from './CreateAgentPage';

export default function CreateAgentDialog({ onClose, onCreated }: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const titleId = useId();
  const scope = useSyncExternalStore(onSurfaceActivated, getActiveSurfaceScope, getActiveSurfaceScope);
  const { workspace } = useCurrentWorkspace();

  return (
    <Dialog open onOpenChange={onClose} size="2xl" aria-labelledby={titleId}>
      <CreateAgentPage
        key={scope.key(scope.epoch, workspace?.id)}
        standalone
        titleId={titleId}
        onCancel={onClose}
        onCreated={() => { onClose(); onCreated(); }}
      />
    </Dialog>
  );
}
