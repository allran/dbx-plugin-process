<script>
  import { loadProcessIcon, peekProcessIcon } from "../lib/iconCache.js";

  let { iconKey = null, name = "", size = 20 } = $props();

  let src = $state("");
  let failed = $state(false);

  $effect(() => {
    const key = iconKey;
    const cached = peekProcessIcon(key);
    if (cached !== undefined) {
      src = cached || "";
      failed = !cached;
      return;
    }
    if (!key) {
      src = "";
      failed = false;
      return;
    }

    let cancelled = false;
    src = "";
    failed = false;
    loadProcessIcon(key).then((dataUrl) => {
      if (cancelled) return;
      src = dataUrl || "";
      failed = !dataUrl;
    });

    return () => {
      cancelled = true;
    };
  });
</script>

{#if src && !failed}
  <img
    class="icon"
    src={src}
    alt=""
    title={name}
    width={size}
    height={size}
    style:width="{size}px"
    style:height="{size}px"
    style:border-radius="{Math.max(4, Math.round(size / 4))}px"
  />
{:else}
  <span
    class="icon placeholder"
    aria-hidden="true"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.max(10, Math.round(size * 0.45))}px"
    style:border-radius="{Math.max(4, Math.round(size / 4))}px"
  >
    {(name || "?").slice(0, 1).toUpperCase()}
  </span>
{/if}

<style>
  .icon {
    flex-shrink: 0;
    object-fit: contain;
    background: color-mix(in srgb, var(--fg) 6%, transparent);
  }

  .placeholder {
    display: grid;
    place-items: center;
    font-weight: 700;
    color: var(--muted);
  }
</style>
