<script lang="ts">
  let { size = 18 }: { size?: number } = $props();
</script>

<!-- The app icon reduced to a glyph: ring gauge with end dot around a battery. -->
<svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
  <path d="M17.6 3.9A9.6 9.6 0 1 0 19.4 18.4" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" />
  <path d="M19.4 18.4A9.6 9.6 0 0 0 20.2 6.4" stroke="currentColor" stroke-opacity="0.3" stroke-width="2.4" stroke-linecap="round" />
  <circle cx="18.6" cy="4.9" r="1.9" fill="currentColor" />
  <rect x="7.2" y="9.4" width="9" height="5.2" rx="1.2" stroke="currentColor" stroke-width="1.6" />
  <rect x="9" y="11.1" width="5.4" height="1.8" rx="0.4" fill="currentColor" />
  <rect x="16.8" y="11" width="1" height="2" rx="0.4" fill="currentColor" />
</svg>
