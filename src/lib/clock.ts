import { readable } from "svelte/store";

/** Current time, ticking every 20 s so countdowns stay fresh without re-rendering constantly. */
export const now = readable(Date.now(), (set) => {
  const id = setInterval(() => set(Date.now()), 20_000);
  return () => clearInterval(id);
});
