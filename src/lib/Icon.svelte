<script lang="ts">
  interface Props { name: string; size?: number; class?: string }
  let { name, size = 20, class: className = '' }: Props = $props();

  const paths: Record<string, string[]> = {
    grid: ['M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z'],
    devices: ['M4 4h16v11H4zM8 21h8M12 15v6', 'M8 8h8M8 11h5'],
    activity: ['M2 12h4l3-8 6 16 3-8h4'],
    bug: ['M8 8h8v7a4 4 0 0 1-8 0V8ZM9 8V6a3 3 0 0 1 6 0v2M12 8v11', 'M5 5l3 3M19 5l-3 3M3 12h5M16 12h5M4 19l4-3M20 19l-4-3'],
    settings: ['M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z', 'M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0'],
    chevron: ['m9 5 7 7-7 7'],
    arrow: ['M4 12h16m-7-7 7 7-7 7'],
    plus: ['M12 5v14M5 12h14'],
    trash: ['M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7'],
    close: ['m6 6 12 12M6 18 18 6'],
    check: ['m5 12 4 4L19 6'],
    usb: ['M12 18V3m-3 3 3-3 3 3M12 14l-5-4V7m5 6 5-3V7', 'M14 20a2 2 0 1 1-4 0 2 2 0 0 1 4 0M5 5h4v3H5zM15 4h4v3h-4z'],
    network: ['M9 3h6v5H9zM3 16h6v5H3zM15 16h6v5h-6zM12 8v4M6 16v-4h12v4'],
    bluetooth: ['M7 7l10 10-5 4V3l5 4L7 17'],
    refresh: ['M20 8a8 8 0 0 0-14-3L3 8m0-5v5h5M4 16a8 8 0 0 0 14 3l3-3m0 5v-5h-5'],
    search: ['M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0m-2 6 6 6'],
    sun: ['M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M5 19l1-1M18 6l1-1'],
    moon: ['M21 13a9 9 0 0 1-10-10A9 9 0 1 0 21 13Z'],
    sliders: ['M4 4v16M12 4v16M20 4v16M2 8h4M10 16h4M18 9h4'],
    save: ['M4 3h13l4 4v14H3V3zM7 3v6h9V3M7 21v-8h10v8'],
    pencil: ['m16 3 5 5-12 12-6 1 1-6L16 3ZM14 5l5 5'],
    'align-left': ['M4 3v18M8 5h12v5H8zM8 14h8v5H8z'],
    'align-center': ['M12 2v20M4 5h16v5H4zM7 14h10v5H7z'],
    'align-right': ['M20 3v18M4 5h12v5H4zM8 14h8v5H8z'],
    'align-top': ['M3 4h18M5 8h5v12H5zM14 8h5v8h-5z'],
    'align-horizontal': ['M2 12h20M5 4h5v16H5zM14 7h5v10h-5z'],
    'align-bottom': ['M3 20h18M5 4h5v12H5zM14 8h5v8h-5z'],
    group: ['M3 3h18v18H3zM6 6h7v7H6zM11 11h7v7h-7z'],
    ungroup: ['M3 3h8v8H3zM13 13h8v8h-8zM16 3h5v5M3 16v5h5'],
    play: ['m8 4 13 8-13 8z'],
    stop: ['M5 5h14v14H5z'],
    info: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M12 11v6M12 7v.1'],
    link: ['M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2'],
    help: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M9 9a3 3 0 1 1 5 2c-2 1-2 2-2 3M12 17v.1'],
    external: ['M14 3h7v7M21 3l-9 9M10 3H3v18h18v-7'],
    wheel: ['M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0M3 10l6 2M21 10l-6 2M12 15v7'],
    joystick: ['M5 18h14l2 3H3l2-3Z', 'M11 18V11L8 8V4l3-2 4 2v5l-2 2v7', 'M9 5h5M16 16h2'],
    pedals: ['M3 20h18M5 20l-2-6 2-9h5l1 9-2 6M15 20l-2-6 2-9h5l1 9-2 6', 'M6 8h1M6 11h1M16 8h1M16 11h1'],
    gamepad: ['M8 7h8c2 0 3 1 4 4l2 6c.5 2-1 4-3 2l-3-3H8l-3 3c-2 2-3.5 0-3-2l2-6c1-3 2-4 4-4Z', 'M7 10v4M5 12h4M16 11h.01M19 13h.01M9 7l1-3h4'],
    throttle: ['M4 19h16v3H4zM8 19V9m8 10V9M5 4h14v5H5z', 'M8 6.5h8M6 14h2M16 14h2'],
    panel: ['M3 4h18v16H3zM6 7h6v4H6z', 'M16 8h2M16 11h2M7 15v2M12 15v2M17 15v2'],
    chip: ['M6 6h12v12H6zM9 9h6v6H9z', 'M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3'],
    command: ['M9 7a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v10a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z'],
    zap: ['m13 2-9 12h7l-1 8 10-12h-7z'],
    copy: ['M9 9h12v12H9zM5 15H3V3h12v2'],
    upload: ['M12 15V3m-5 5 5-5 5 5M3 16v5h18v-5'],
    back: ['M20 12H4m7-7-7 7 7 7'],
    undo: ['M3 10h11a7 7 0 0 1 7 7M8 5l-5 5 5 5'],
    redo: ['M21 10H10a7 7 0 0 0-7 7M16 5l5 5-5 5'],
    download: ['M12 3v12m-5-5 5 5 5-5M3 16v5h18v-5'],
    lock: ['M5 10h14v11H5zM8 10V6a4 4 0 0 1 8 0v4M12 14v3'],
    alert: ['m12 3 10 18H2zM12 9v5M12 17v.1'],
    mobile: ['M7 2h10v20H7zM11 18h2'],
    monitor: ['M3 3h18v14H3zM8 21h8M12 17v4'],
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" class={className} fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#each paths[name] ?? paths.info as d (d)}
    <path {d} />
  {/each}
</svg>
