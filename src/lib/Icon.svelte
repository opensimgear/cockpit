<script lang="ts">
  interface Props { name: string; size?: number; class?: string }
  let { name, size = 20, class: className = '' }: Props = $props();

  const paths: Record<string, string[]> = {
    grid: ['M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z'],
    devices: ['M4 4h16v11H4zM8 21h8M12 15v6', 'M8 8h8M8 11h5'],
    activity: ['M2 12h4l3-8 6 16 3-8h4'],
    settings: ['M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z', 'M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0'],
    chevron: ['m9 5 7 7-7 7'],
    arrow: ['M4 12h16m-7-7 7 7-7 7'],
    plus: ['M12 5v14M5 12h14'],
    close: ['m6 6 12 12M6 18 18 6'],
    check: ['m5 12 4 4L19 6'],
    usb: ['M12 18V3m-3 3 3-3 3 3M12 14l-5-4V7m5 6 5-3V7', 'M14 20a2 2 0 1 1-4 0 2 2 0 0 1 4 0M5 5h4v3H5zM15 4h4v3h-4z'],
    network: ['M9 3h6v5H9zM3 16h6v5H3zM15 16h6v5h-6zM12 8v4M6 16v-4h12v4'],
    refresh: ['M20 8a8 8 0 0 0-14-3L3 8m0-5v5h5M4 16a8 8 0 0 0 14 3l3-3m0 5v-5h-5'],
    search: ['M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0m-2 6 6 6'],
    sun: ['M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M5 19l1-1M18 6l1-1'],
    moon: ['M21 13a9 9 0 0 1-10-10A9 9 0 1 0 21 13Z'],
    sliders: ['M4 4v16M12 4v16M20 4v16M2 8h4M10 16h4M18 9h4'],
    save: ['M4 3h13l4 4v14H3V3zM7 3v6h9V3M7 21v-8h10v8'],
    play: ['m8 4 13 8-13 8z'],
    stop: ['M5 5h14v14H5z'],
    info: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M12 11v6M12 7v.1'],
    link: ['M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2'],
    help: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M9 9a3 3 0 1 1 5 2c-2 1-2 2-2 3M12 17v.1'],
    external: ['M14 3h7v7M21 3l-9 9M10 3H3v18h18v-7'],
    wheel: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0M3 10l6 2M21 10l-6 2M12 15v7'],
    command: ['M9 7a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v10a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z'],
    zap: ['m13 2-9 12h7l-1 8 10-12h-7z'],
    copy: ['M9 9h12v12H9zM5 15H3V3h12v2'],
    download: ['M12 3v12m-5-5 5 5 5-5M3 16v5h18v-5'],
    lock: ['M5 10h14v11H5zM8 10V6a4 4 0 0 1 8 0v4M12 14v3'],
    alert: ['m12 3 10 18H2zM12 9v5M12 17v.1'],
    monitor: ['M3 3h18v14H3zM8 21h8M12 17v4'],
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" class={className} fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#each paths[name] ?? paths.info as d (d)}
    <path {d} />
  {/each}
</svg>
