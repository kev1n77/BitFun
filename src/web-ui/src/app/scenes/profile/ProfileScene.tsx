import React from 'react';
import './ProfileScene.scss';

interface ProfileSceneProps {
  /** Legacy prop – preserved for compatibility; nursery manages its own navigation */
  workspacePath?: string;
}

const ProfileScene: React.FC<ProfileSceneProps> = () => (
  <div className="bitfun-profile-scene">
    <div className="bitfun-scene-viewport__clip bitfun-scene-viewport__clip--empty">
      <p className="bitfun-scene-viewport__empty-hint">This area is unavailable.</p>
    </div>
  </div>
);

export default ProfileScene;
