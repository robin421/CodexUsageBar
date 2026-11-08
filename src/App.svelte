<script lang="ts">
  import { onMount } from "svelte";
  import Panel from "./components/Panel.svelte";
  import Settings from "./components/Settings.svelte";
  import { api } from "./lib/api";
  import { lang } from "./lib/i18n";
  import type { AppSnapshot } from "./lib/types";

  let snap: AppSnapshot | null = $state(null);
  let route = $state(location.hash);
  let systemDark = $state(matchMedia("(prefers-color-scheme: dark)").matches);

  onMount(() => {
    api.getSnapshot().then((s) => (snap = s));
    const unlisten = api.onSnapshot((s) => (snap = s));
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const onScheme = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener("change", onScheme);
    const onHash = () => (route = location.hash);
    addEventListener("hashchange", onHash);
    return () => {
      unlisten.then((f) => f());
      mq.removeEventListener("change", onScheme);
      removeEventListener("hashchange", onHash);
    };
  });

  $effect(() => {
    if (!snap) return;
    lang.set(snap.settings.resolved_language);
    document.documentElement.lang = snap.settings.resolved_language === "zh" ? "zh-CN" : "en";
    const theme = snap.settings.theme === "system" ? (systemDark ? "dark" : "light") : snap.settings.theme;
    document.documentElement.dataset.theme = theme;
  });
</script>

{#if snap}
  {#if route.startsWith("#/settings")}
    <Settings {snap} />
  {:else}
    <Panel {snap} />
  {/if}
{/if}
