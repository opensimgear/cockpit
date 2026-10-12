import { invoke, isTauri } from '@tauri-apps/api/core';

const download = document.querySelector<HTMLAnchorElement>('#firmware-download');
const status = document.querySelector<HTMLParagraphElement>('#firmware-status');

if (isTauri() && download && status) {
  let saving = false;
  download.removeAttribute('download');
  download.addEventListener('click', async event => {
    event.preventDefault();
    if (saving) return;
    saving = true;
    download.setAttribute('aria-disabled', 'true');
    download.setAttribute('aria-busy', 'true');
    status.hidden = false;
    status.dataset.error = 'false';
    status.textContent = 'Saving gateway firmware to Downloads…';
    try {
      const path = await invoke<string>('export_pico_firmware');
      status.textContent = `Saved firmware to ${path}. Copy this file onto RPI-RP2.`;
    } catch (error) {
      status.dataset.error = 'true';
      status.textContent = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
      download.removeAttribute('aria-disabled');
      download.removeAttribute('aria-busy');
    }
  });
}
