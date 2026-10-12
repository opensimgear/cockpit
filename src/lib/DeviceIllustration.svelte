<script lang="ts">
  interface Props { kind: 'shifter' | 'pedals' | 'gateway' | 'wheel' | 'handbrake' | 'panel'; compact?: boolean }
  let { kind, compact = false }: Props = $props();
  const uid = $props.id();
  const labels = {
    shifter: 'Isometric sequential shifter with graphite enclosure, silver shaft and mint grip',
    pedals: 'Isometric three pedal assembly with adjustable metal pedal plates',
    gateway: 'Isometric dual CAN gateway with USB connection and status indicators',
    wheel: 'Isometric racing steering wheel with mint accents and center display',
    handbrake: 'Isometric handbrake with tall lever and mint grip',
    panel: 'Isometric button panel with rotary encoders and illuminated buttons',
  };
</script>

<svg viewBox="0 0 400 230" class:compact role="img" aria-label={labels[kind]}>
  <title>{labels[kind]}</title>
  <defs>
    <linearGradient id={`${uid}-steel`} x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#69736d" /><stop offset=".35" stop-color="#d3ddd5" /><stop offset=".58" stop-color="#9ba89e" /><stop offset="1" stop-color="#465249" />
    </linearGradient>
    <linearGradient id={`${uid}-face`} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#363e38" /><stop offset="1" stop-color="#171c19" />
    </linearGradient>
    <linearGradient id={`${uid}-grip`} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#b9e7c2" /><stop offset="1" stop-color="#669e73" />
    </linearGradient>
    <radialGradient id={`${uid}-ground`}>
      <stop offset="0" stop-color="#000" stop-opacity=".45" /><stop offset="1" stop-color="#000" stop-opacity="0" />
    </radialGradient>
  </defs>

  {#if !compact}
    <g fill="none" stroke="#a0d9ab" stroke-width=".6" opacity=".075">
      {#each [-2, -1, 0, 1, 2, 3, 4, 5] as line (line)}
        <path d={`M${40 + line * 36} 220l225-100`} /><path d={`M${40 + line * 36} 120l225 100`} />
      {/each}
    </g>
    <path d="M57 185 199 121l144 64-142 64Z" fill="none" stroke="#a0d9ab" stroke-opacity=".09" stroke-width=".7" />
  {/if}
  <ellipse cx="208" cy="194" rx="150" ry="29" fill={`url(#${uid}-ground)`} />

  {#if kind === 'shifter'}
    <g transform="translate(0,-7)" stroke-linejoin="round">
      <path d="m121 175 96-37 88 36-98 40Z" fill="#101511" stroke="#435047" stroke-width="1.1" />
      <path d="m135 145 82-32 70 28-81 34Z" fill="#3a433c" stroke="#58695d" stroke-width="1.2" />
      <path d="m135 145 71 30v37l-71-31Z" fill={`url(#${uid}-face)`} stroke="#455249" stroke-width="1.1" />
      <path d="m206 175 81-34v38l-81 33Z" fill="#19201b" stroke="#3e4e43" stroke-width="1.1" />
      <path d="m138 146 68 28 79-33" fill="none" stroke="#a0d9ab" stroke-opacity=".55" stroke-width="1.2" />
      <path d="m149 152 57 23v10l-57-24Z" fill="#26332a" />
      <path d="m154 153 23 9" stroke="#a0d9ab" stroke-width="2" opacity=".75" />
      <path d="m151 176 40 16" stroke="#6f8074" stroke-width=".7" opacity=".4" />
      <path d="m151 180 24 10" stroke="#6f8074" stroke-width=".7" opacity=".4" />
      <path d="m213 183 19-8v13l-19 8Z" fill="#070c08" stroke="#59655b" stroke-width="1" />
      <path d="m240 172 19-8v13l-19 8Z" fill="#070c08" stroke="#59655b" stroke-width="1" />
      <path d="m217 184 11-5m-11 9 11-5m16-11 11-5m-11 9 11-5" stroke="#708278" stroke-width=".9" />
      <path d="m211 204 18-7m9-4 18-7" stroke="#a0d9ab" stroke-width=".8" opacity=".3" />
      <ellipse cx="206" cy="140" rx="28" ry="12" fill="#151d17" stroke="#59685e" />
      <ellipse cx="206" cy="137" rx="20" ry="9" fill="#333e36" stroke="#7d9082" stroke-width=".8" />
      <ellipse cx="206" cy="135" rx="12" ry="5" fill="#101a13" />
      <path d="M201 74h10v61c0 4-10 4-10 0Z" fill={`url(#${uid}-steel)`} stroke="#9baba0" stroke-width=".6" />
      <path d="M204 78v50" stroke="#dae8dc" stroke-width="1.1" opacity=".7" />
      <path d="M193 48c0-8 27-8 27 0l-2 34c-1 9-24 9-24 0Z" fill="#202e24" stroke="#6d9277" stroke-width="1.1" />
      <path d="M193 48c0-8 27-8 27 0l-1 12c-2 7-23 7-26 0Z" fill={`url(#${uid}-grip)`} stroke="#b8e5c0" stroke-width=".8" />
      <ellipse cx="206.5" cy="47" rx="13.5" ry="6" fill="#b4dfbd" stroke="#d3edd8" stroke-width=".8" />
      <path d="m201 48 5-3 6 3-5 2Z" fill="#285b36" opacity=".75" />
      <path d="M198 65v14m4-13v14m4-13v14m4-14v14m4-15v13" stroke="#718e79" stroke-width=".7" opacity=".55" />
      <g fill="#111914" stroke="#8b9d91" stroke-width=".7">
        <ellipse cx="148" cy="145" rx="3" ry="1.6" /><ellipse cx="213" cy="122" rx="3" ry="1.6" /><ellipse cx="273" cy="142" rx="3" ry="1.6" /><ellipse cx="206" cy="168" rx="3" ry="1.6" />
      </g>
      <path d="m146 145 4 0m61-23h4m56 20h4m-71 26h4" stroke="#a0d9ab" stroke-width=".6" opacity=".8" />
      <path d="m126 176 8-3v7l-8 3Zm168-1 8-3v7l-8 3Z" fill="#3b4840" stroke="#7d9483" stroke-width=".6" />
      <path d="m283 170 21 7 14-5" fill="none" stroke="#5e7465" stroke-width="2" />
    </g>
  {:else if kind === 'pedals'}
    <g stroke-linejoin="round">
      <path d="m82 167 145-59 104 65-145 59Z" fill="#19221c" stroke="#536159" />
      <path d="m82 167 104 65v-12L82 155Z" fill="#303b33" stroke="#536159" />
      <path d="m82 155 145-59 104 65-145 59Z" fill="#2a342d" stroke="#698471" />
      <path d="m96 160 130-52 87 53-128 52Z" fill="#151d18" stroke="#405447" />
      {#each [0, 1, 2] as i (i)}
        <g transform={`translate(${i * 52},${i * -19})`}>
          <path d="m129 165 15-63 19-6-12 76Z" fill="#536058" stroke="#89968d" />
          <path d="m125 160 11-56 10 1-8 57Z" fill="#202d24" stroke="#657a6b" />
          <path d="m116 77 34-13 26 17-34 15Z" fill="#80958a" stroke="#b4c7ba" />
          <path d="m116 77 26 19-8 47-27-18Z" fill={`url(#${uid}-steel)`} stroke="#a6b8aa" />
          <path d="m142 96 34-15-8 47-34 15Z" fill="#34463b" stroke="#718d79" />
          <path d="m122 84 13 9m-16 2 13 9m-15 1 13 9m-15 1 13 9" stroke="#263c2e" stroke-width="2" />
          <path d="m146 100 23-10m-25 20 23-10m-25 20 23-10m-25 20 23-10" stroke="#a0d9ab" stroke-width="1.3" opacity=".5" />
          <path d="m141 156 13 9 4-13-12-8Z" fill="#658a6f" stroke="#a0d9ab" />
          <ellipse cx="140" cy="176" rx="12" ry="5" fill="#263c2e" stroke="#94bc9c" />
        </g>
      {/each}
      <path d="m94 160 91 57 134-55" fill="none" stroke="#a0d9ab" stroke-opacity=".65" />
    </g>
  {:else if kind === 'gateway'}
    <g stroke-linejoin="round">
      <path d="m117 128 110-43 79 44-111 45Z" fill="#3e4b41" stroke="#798b7c" />
      <path d="m117 128 78 46v39l-78-46Z" fill={`url(#${uid}-face)`} stroke="#516758" />
      <path d="m195 174 111-45v38l-111 46Z" fill="#19241d" stroke="#506554" />
      <path d="m121 130 74 44 107-44" fill="none" stroke="#a0d9ab" stroke-opacity=".75" />
      <path d="m155 126 51-20 32 18-51 20Z" fill="#192a1e" stroke="#72957c" />
      <path d="m174 126 18-7 13 7-19 7Z" fill="#a0d9ab" opacity=".7" />
      <path d="m128 158 27 16v17l-27-16Z" fill="#111914" stroke="#91a197" />
      <path d="m133 161 17 10v11l-17-10Z" fill="#87978d" />
      <path d="m211 180 31-12v17l-31 13Z" fill="#101914" stroke="#87998d" />
      <path d="m256 162 31-12v17l-31 13Z" fill="#101914" stroke="#87998d" />
      <path d="m215 180 23-9m-23 13 23-9m22-13 23-9m-23 13 23-9" stroke="#607769" stroke-width="1.2" />
      <ellipse cx="175" cy="179" rx="2.4" ry="3.8" fill="#a0d9ab" /><ellipse cx="184" cy="184" rx="2.4" ry="3.8" fill="#44654e" />
      <g fill="#18241c" stroke="#94ac9c" stroke-width=".7"><ellipse cx="129" cy="129" rx="3" ry="1.6" /><ellipse cx="225" cy="93" rx="3" ry="1.6" /><ellipse cx="292" cy="131" rx="3" ry="1.6" /><ellipse cx="194" cy="168" rx="3" ry="1.6" /></g>
      <path d="m118 179-27-17-19 8" fill="none" stroke="#435d4b" stroke-width="4" />
    </g>
  {:else if kind === 'wheel'}
    <g transform="translate(200,113) rotate(-10)" stroke-linejoin="round">
      <ellipse cx="7" cy="10" rx="83" ry="76" fill="#131c16" stroke="#394f40" stroke-width="16" />
      <ellipse rx="83" ry="76" fill="none" stroke="#54695b" stroke-width="15" />
      <path d="M-80-15A83 76 0 0 1-47-63M47-63A83 76 0 0 1 80-15" fill="none" stroke="#a0d9ab" stroke-width="16" />
      <path d="m-75-3 42-25 68 0 40 25-14 30-38 2-8 44-30 0-8-44-38-2Z" fill="#27382d" stroke="#759b80" />
      <path d="m-55-10 23-12h66l23 12-9 26-33 2-7 37H-8l-7-37-32-2Z" fill="#15251b" stroke="#456d50" />
      <path d="M-21-21h42V4h-42Z" fill="#071a0d" stroke="#77a785" /><path d="M-13-9h9m8 0h9" stroke="#a0d9ab" stroke-width="2" />
      <ellipse cy="25" rx="11" ry="10" fill="#334d3b" stroke="#81af8e" />
      <path d="m-5 24 5-3 5 3-5 3Z" fill="#a0d9ab" />
      {#each [-46, 46] as x (x)}
        <circle cx={x} cy="2" r="5" fill="#a0d9ab" /><circle cx={x} cy="17" r="4" fill="#506e58" />
      {/each}
      <path d="M0-83v15" stroke="#c0e5c8" stroke-width="6" />
    </g>
  {:else if kind === 'handbrake'}
    <g stroke-linejoin="round">
      <path d="m113 177 96-35 86 34-97 39Z" fill="#26382c" stroke="#5f7d67" />
      <path d="m113 177 85 38v-19l-85-38Z" fill="#1e2b22" stroke="#4c6855" />
      <path d="m113 158 96-35 86 34-97 39Z" fill="#364b3d" stroke="#7c9b83" />
      <path d="m169 168 26-85 23-7 20 82-35 14Z" fill="#23382a" stroke="#7a9782" />
      <path d="m195 83 23-7 20 82-35 14Z" fill="#4d6656" stroke="#7a9782" />
      <path d="m206 140 21-96 9 3-18 96Z" fill={`url(#${uid}-steel)`} stroke="#b6cbbd" />
      <path d="m225 58 10-39c2-7 18-3 16 4l-10 39c-2 7-18 3-16-4Z" fill={`url(#${uid}-grip)`} stroke="#bedec6" />
      <ellipse cx="239" cy="21" rx="8" ry="4" fill="#c5e6cc" transform="rotate(14 239 21)" />
      <ellipse cx="207" cy="151" rx="8" ry="13" fill="#172d1f" stroke="#9ccda8" /><ellipse cx="207" cy="151" rx="3" ry="5" fill="#a0d9ab" />
      <path d="m182 169 17-64" stroke="#a0d9ab" stroke-opacity=".45" />
      <path d="m133 165 8 3m73-34 8 3m51 29 8 3m-90 18 8 3" stroke="#a0d9ab" stroke-width="2" />
    </g>
  {:else}
    <g stroke-linejoin="round">
      <path d="m102 128 101-61 102 49-100 71Z" fill="#3d5444" stroke="#83a68e" />
      <path d="m102 128 103 59v25l-103-60Z" fill="#23372a" stroke="#52725b" />
      <path d="m205 187 100-71v25l-100 71Z" fill="#172c1e" stroke="#52725b" />
      <path d="m115 126 89-52 88 43-88 62Z" fill="#192d20" stroke="#729e7d" />
      <path d="m197 93 43 23-26 19-43-24Z" fill="#061b0e" stroke="#6aa079" /><path d="m187 109 18 10 12-7" fill="none" stroke="#a0d9ab" stroke-width="2" />
      {#each [0, 1, 2] as row (row)}
        {#each [0, 1, 2] as col (col)}
          <ellipse cx={144 + col * 27 + row * 18} cy={126 + col * 14 - row * 11} rx="8" ry="5" fill={(row + col) % 3 === 0 ? '#a0d9ab' : '#3a5c45'} stroke="#95bfa0" stroke-width=".7" />
        {/each}
      {/each}
      <ellipse cx="252" cy="129" rx="11" ry="7" fill="#121f16" stroke="#a0d9ab" /><ellipse cx="252" cy="125" rx="11" ry="7" fill="#567660" stroke="#a0d9ab" /><path d="m252 125 5-3" stroke="#c1dfc7" />
      <ellipse cx="232" cy="146" rx="11" ry="7" fill="#121f16" stroke="#a0d9ab" /><ellipse cx="232" cy="142" rx="11" ry="7" fill="#567660" stroke="#a0d9ab" /><path d="m232 142 5-3" stroke="#c1dfc7" />
      <path d="m107 127 98 57 96-66" fill="none" stroke="#a0d9ab" stroke-opacity=".7" />
    </g>
  {/if}
</svg>

<style>
  svg { display: block; width: 100%; height: 100%; overflow: visible; }
  svg.compact { overflow: hidden; }
</style>
