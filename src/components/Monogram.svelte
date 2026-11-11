<script lang="ts">
  import { MONOGRAM } from "../lib/format";
  import { BRAND_ICONS } from "../lib/brand_icons";
  import type { ProviderId } from "../lib/types";

  let { id, size = 22 }: { id: ProviderId; size?: number } = $props();

  const brand = $derived(BRAND_ICONS[id]);
  const fallback = $derived(MONOGRAM[id] ?? id.slice(0, 2).toUpperCase());
</script>

<span
  class="mono brand-{id}"
  class:is-svg={!!brand}
  style:--size="{size}px"
  aria-hidden="true"
>
  {#if brand}
    <svg
      class="brand-icon"
      viewBox={brand.viewBox}
      fill="currentColor"
      aria-hidden="true"
    >
      {#each brand.paths as d}
        <path {d} />
      {/each}
    </svg>
  {:else}
    <span class="mono-fallback">{fallback}</span>
  {/if}
</span>

<style>
  .mono {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: var(--size);
    height: var(--size);
    border-radius: calc(var(--size) * 0.28);
    background: var(--inset);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--ink);
    font-size: calc(var(--size) * 0.46);
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 1;
    overflow: hidden;
    user-select: none;
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }

  .brand-icon {
    display: block;
    width: calc(var(--size) * 0.64);
    height: calc(var(--size) * 0.64);
    pointer-events: none;
  }

  /* Official brand subtle color accents (light mode) */
  .brand-claude { color: #d97757; }
  .brand-codex { color: #10a37f; }
  .brand-agy, .brand-gemini { color: #3b82f6; }
  .brand-copilot { color: #7c3aed; }
  .brand-deepseek { color: #0066ff; }
  .brand-jetbrains { color: #f05138; }
  .brand-openrouter { color: #6366f1; }
  .brand-moonshot { color: #0284c7; }
  .brand-zai { color: #2563eb; }
  .brand-minimax { color: #f43f5e; }
  .brand-opencode { color: #10b981; }
  .brand-codebuff { color: #16a34a; }

  /* grok and cursor keep pure crisp monochrome currentColor */
  .brand-grok, .brand-cursor { color: var(--ink); }

  /* Official brand subtle color accents (dark mode adjustments for luminosity & contrast) */
  :global(:root[data-theme="dark"]) .brand-claude { color: #e0886c; }
  :global(:root[data-theme="dark"]) .brand-codex { color: #10b981; }
  :global(:root[data-theme="dark"]) .brand-agy,
  :global(:root[data-theme="dark"]) .brand-gemini { color: #60a5fa; }
  :global(:root[data-theme="dark"]) .brand-copilot { color: #a78bfa; }
  :global(:root[data-theme="dark"]) .brand-deepseek { color: #3b82f6; }
  :global(:root[data-theme="dark"]) .brand-jetbrains { color: #fb7185; }
  :global(:root[data-theme="dark"]) .brand-openrouter { color: #818cf8; }
  :global(:root[data-theme="dark"]) .brand-moonshot { color: #38bdf8; }
  :global(:root[data-theme="dark"]) .brand-zai { color: #60a5fa; }
  :global(:root[data-theme="dark"]) .brand-minimax { color: #fb7185; }
  :global(:root[data-theme="dark"]) .brand-opencode { color: #34d399; }
  :global(:root[data-theme="dark"]) .brand-codebuff { color: #4ade80; }
  :global(:root[data-theme="dark"]) .brand-grok,
  :global(:root[data-theme="dark"]) .brand-cursor { color: var(--ink); }
</style>
