use remote_server::manager::{RemoteServerManager, RemoteServerManagerEvent};
use warp_core::features::FeatureFlag;
use warp_util::host_id::HostId;
use warpui::{AppContext, ModelContext, SingletonEntity};

use super::bundled::BundledSkill;
use super::SkillManager;

pub(crate) fn wire_remote_bundled_skills(ctx: &mut AppContext) {
    SkillManager::handle(ctx).update(ctx, |manager, ctx| {
        manager.subscribe_to_remote_bundled_skills(ctx);
    });
}

impl SkillManager {
    fn subscribe_to_remote_bundled_skills(&mut self, ctx: &mut ModelContext<Self>) {
        let remote_server_manager = RemoteServerManager::handle(ctx);
        ctx.subscribe_to_model(&remote_server_manager, |me, event, ctx| match event {
            RemoteServerManagerEvent::HostConnected { host_id } => {
                me.bootstrap_remote_bundled_skill(host_id.clone(), ctx);
            }
            RemoteServerManagerEvent::HostDisconnected { host_id } => {
                me.remove_remote_bundled_skill(host_id);
            }
            RemoteServerManagerEvent::SessionConnecting { .. }
            | RemoteServerManagerEvent::SessionConnected { .. }
            | RemoteServerManagerEvent::SessionConnectionFailed { .. }
            | RemoteServerManagerEvent::SessionDisconnected { .. }
            | RemoteServerManagerEvent::SessionReconnected { .. }
            | RemoteServerManagerEvent::SessionDeregistered { .. }
            | RemoteServerManagerEvent::NavigatedToDirectory { .. }
            | RemoteServerManagerEvent::RepoMetadataSnapshot { .. }
            | RemoteServerManagerEvent::RepoMetadataUpdated { .. }
            | RemoteServerManagerEvent::RepoMetadataDirectoryLoaded { .. }
            | RemoteServerManagerEvent::CodebaseIndexStatusesSnapshot { .. }
            | RemoteServerManagerEvent::CodebaseIndexStatusUpdated { .. }
            | RemoteServerManagerEvent::BufferUpdated { .. }
            | RemoteServerManagerEvent::BufferConflictDetected { .. }
            | RemoteServerManagerEvent::DiffStateSnapshotReceived { .. }
            | RemoteServerManagerEvent::DiffStateMetadataUpdateReceived { .. }
            | RemoteServerManagerEvent::DiffStateFileDeltaReceived { .. }
            | RemoteServerManagerEvent::GetBranchesResponse { .. }
            | RemoteServerManagerEvent::SetupStateChanged { .. }
            | RemoteServerManagerEvent::BinaryCheckComplete { .. }
            | RemoteServerManagerEvent::BinaryInstallComplete { .. }
            | RemoteServerManagerEvent::ClientRequestFailed { .. }
            | RemoteServerManagerEvent::CodebaseIndexMutationFailed { .. }
            | RemoteServerManagerEvent::ServerMessageDecodingError { .. } => {}
        });

        let connected_host_ids = remote_server_manager
            .as_ref(ctx)
            .connected_host_ids()
            .cloned()
            .collect::<Vec<_>>();
        for host_id in connected_host_ids {
            self.bootstrap_remote_bundled_skill(host_id, ctx);
        }
    }

    fn bootstrap_remote_bundled_skill(&mut self, host_id: HostId, ctx: &mut ModelContext<Self>) {
        if !FeatureFlag::BundledSkills.is_enabled() {
            return;
        }

        let Some(bootstrap) = self.begin_remote_bundled_skill_bootstrap(host_id) else {
            return;
        };

        // Catalog discovery remains client-owned. Delivering any supporting
        // resources to the execution host is a separate lifecycle.
        ctx.spawn(BundledSkill::detect(), move |me, bundled_skill, _| {
            me.complete_remote_bundled_skill_bootstrap(bootstrap, bundled_skill);
        });
    }
}
