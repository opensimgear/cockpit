<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import ProfileDebugView from './ProfileDebugView.svelte';
  import { gatewayRequest } from './gateway';
  import { ProfilesClient, captureProfileSummaries, type DeepReadonly, type ProfileSummary } from './profiles';
  import { ProfileInventoryClient, type ResourceSummary } from './profile-inventory';
  import { ProfileCompilerClient } from './profile-compilation';
  import type { ProfileRuntimeController, RuntimeControlState } from './profile-runtime-controller';
  import { ProfileInspectorClient } from './profile-inspection';
  import { ProfileInspectionController } from './profile-inspection-controller';
  import { ProfileTopologyClient } from './profile-activity';
  import { ProfileActivityController } from './profile-activity-controller';

  let { runtime, runtimeState }: {
    runtime: ProfileRuntimeController;
    runtimeState: DeepReadonly<RuntimeControlState>;
  } = $props();
  const profiles = new ProfilesClient(async request => {
    const result = (await gatewayRequest(request)).profiles;
    if (!result) throw new Error('Profiles are unavailable. Reconnect the native service.');
    return result;
  });
  const resources = new ProfileInventoryClient(async request => {
    const result = (await gatewayRequest(request)).profileResources;
    if (!result) throw new Error('Resource inventory is unavailable. Reconnect the native service.');
    return result;
  });
  const compiler = new ProfileCompilerClient(async request => {
    const result = (await gatewayRequest(request)).profileCompilation;
    if (!result) throw new Error('Native compilation is unavailable. Reconnect the service.');
    return result;
  });
  const inspectionClient = new ProfileInspectorClient(async request => {
    const result = (await gatewayRequest(request)).profileInspection;
    if (!result) throw new Error('Native applied port inspection is unavailable. Reconnect the service.');
    return result;
  });
  const inspection = new ProfileInspectionController(inspectionClient);
  const activity = new ProfileActivityController(new ProfileTopologyClient(async request => {
    const result = (await gatewayRequest(request)).profileTopology;
    if (!result) throw new Error('Native applied topology is unavailable. Reconnect the service.');
    return result;
  }), inspectionClient);
  let savedProfiles = $state.raw<readonly DeepReadonly<ProfileSummary>[]>([]);
  let inventory = $state.raw<readonly DeepReadonly<ResourceSummary>[]>([]);
  let inventoryRevision = $state(0), profileError = $state(''), inventoryError = $state('');
  let inventorySignature = '', refreshing = false, disposed = false;
  const profileName = $derived(savedProfiles.find(profile => profile.id === runtimeState.status?.appliedSnapshot?.profileId)?.name);

  async function refresh() {
    if (refreshing || disposed || document.hidden) return;
    refreshing = true;
    try {
      await Promise.all([
        profiles.list().then(result => {
          if (disposed) return;
          savedProfiles = captureProfileSummaries(result.profiles);
          profileError = '';
        }).catch(error => { if (!disposed) profileError = error instanceof Error ? error.message : String(error); }),
        resources.listAll().then(result => {
          if (disposed) return;
          const signature = JSON.stringify([result.session, result.revision, result.resources]);
          if (signature !== inventorySignature) { inventorySignature = signature; inventoryRevision++; }
          inventory = result.resources;
          inventoryError = result.inventoryDiagnostics?.join(' ') ?? '';
        }).catch(error => { if (!disposed) inventoryError = error instanceof Error ? error.message : String(error); }),
      ]);
    } finally { refreshing = false; }
  }
  onMount(() => {
    const update = () => { void refresh(); };
    update();
    const timer = window.setInterval(update, 3000);
    document.addEventListener('visibilitychange', update);
    window.addEventListener('focus', update);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', update);
      window.removeEventListener('focus', update);
    };
  });
  onDestroy(() => { disposed = true; inspection.dispose(); activity.dispose(); });
</script>

{#if profileError || inventoryError}<p class="debug-context-error" role="alert">{[profileError, inventoryError].filter(Boolean).join(' ')}</p>{/if}
<ProfileDebugView profiles={savedProfiles} {inventory} {inventoryRevision} {compiler} {runtimeState} {runtime} {inspection} {activity} {profileName} loadProfile={id => profiles.getAssignments(id)}/>

<style>
  .debug-context-error { color: var(--muted); font-size: var(--ui-text-size); line-height: var(--ui-line-height); margin-bottom: var(--ui-section-gap); }
</style>
