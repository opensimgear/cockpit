import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { installNumericInputWheel } from './lib/numeric-input-wheel';

const target = document.getElementById('app')!;
installNumericInputWheel(target);
mount(App, { target });
