<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import ProfileDebugView from './ProfileDebugView.svelte';
  import { gatewayRequest } from './gateway';
  import { ProfilesClient, type DeepReadonly, type ProfileSummary } from './profiles';
  import { observeProfileLibrary } from './profile-read-models/shared';
  import type { ResourceSummary } from './profile-inventory';
  import { observeProfileResources } from './profile-resources-shared';
  import { ProfileCompilerClient } from './profile-compilation';
  import type { ProfileRuntimeController, RuntimeControlState } from './profile-runtime-controller';
  import { ProfileInspectorClient } from './profile-inspection';
  import { ProfileInspectionController } from './profile-inspection-controller';
  import { observeProfileNodeSamples } from './profile-samples/shared';
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
  const inspection = new ProfileInspectionController(inspectionClient, observeProfileNodeSamples);
  const activity = new ProfileActivityController(new ProfileTopologyClient(async request => {
    const result = (await gatewayRequest(request)).profileTopology;
    if (!result) throw new Error('Native applied topology is unavailable. Reconnect the service.');
    return result;
  }), observeProfileNodeSamples);
  let savedProfiles = $state.raw<readonly DeepReadonly<ProfileSummary>[]>([]);
  let inventory = $state.raw<readonly DeepReadonly<ResourceSummary>[]>([]);
  let inventoryRevision = $state(0), profileError = $state(''), inventoryError = $state('');
  let inventorySignature = '';
  const profileName = $derived(savedProfiles.find(profile => profile.id === runtimeState.status?.appliedSnapshot?.profileId)?.name);

  onMount(() => {
    const stopLibrary = observeProfileLibrary(state => {
      if (state.status === 'ready' && state.value) {
        savedProfiles = state.value.profiles;
        profileError = '';
      } else if (state.status !== 'connecting') {
        profileError = state.error ?? 'Waiting for the current profile library.';
      }
    });
    const stopResources = observeProfileResources((page, error) => {
      if (!page) {
        inventoryError = error ?? 'Waiting for the current resource inventory.';
        return;
      }
      const signature = JSON.stringify([page.session, page.revision]);
      if (signature !== inventorySignature) {
        inventorySignature = signature;
        inventoryRevision++;
        inventory = page.resources;
      }
      inventoryError = page.inventoryDiagnostics?.join(' ') ?? '';
    });
    return () => {
      stopResources();
      stopLibrary();
    };
  });
  onDestroy(() => { inspection.dispose(); activity.dispose(); });
</script>

{#if profileError || inventoryError}<p class="debug-context-error" role="alert">{[profileError, inventoryError].filter(Boolean).join(' ')}</p>{/if}
<ProfileDebugView profiles={savedProfiles} {inventory} {inventoryRevision} {compiler} {runtimeState} {runtime} {inspection} {activity} {profileName} loadProfile={id => profiles.getAssignments(id)}/>

<style>
  .debug-context-error { color: var(--muted); font-size: var(--ui-text-size); line-height: var(--ui-line-height); margin-bottom: var(--ui-section-gap); }
</style>
