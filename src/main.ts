import { mount } from 'svelte';
import App from './App.svelte';
import ProfileActivationWindow from './lib/ProfileActivationWindow.svelte';
import './app.css';
import { installNumericInputWheel } from './lib/numeric-input-wheel';
import { bootstrapUiPreferences } from './lib/persistence/native-ui-preferences';

const target = document.getElementById('app')!;
async function start() {
  await bootstrapUiPreferences();
  installNumericInputWheel(target);
  mount(new URLSearchParams(window.location.search).has('profile-activation-window') ? ProfileActivationWindow : App, { target });
}
void start();
