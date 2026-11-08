<script lang="ts">
  let { checked, label, disabled = false, onchange }: { checked: boolean; label: string; disabled?: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button type="button" role="switch" class="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 36px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 99px;
    background: var(--line-strong);
    transition: background var(--dur) var(--ease);
  }
  .switch[aria-checked="true"] { background: var(--ink); }
  .switch:disabled { opacity: 0.45; cursor: default; }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: 0 1px 2px rgb(0 0 0 / 25%);
    transition: transform var(--dur) var(--ease);
  }
  .switch[aria-checked="true"] .knob { transform: translateX(14px); }
  :global([data-theme="dark"]) .switch[aria-checked="true"] .knob { background: var(--on-ink); }
</style>
