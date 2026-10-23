<script>
  import { onDestroy, onMount } from "svelte";
  import ProcessDetailModal from "./components/ProcessDetailModal.svelte";
  import ProcessIcon from "./components/ProcessIcon.svelte";
  import Toast from "./components/Toast.svelte";
  import { formatBytes, formatCpu, normalizePorts, processMatches } from "./lib/format.js";
  import { resolveMessages } from "./lib/i18n.js";

  let text = $state(resolveMessages("en"));
  let items = $state([]);
  let summary = $state(null);
  let query = $state("");
  let bootError = $state("");
  let refreshing = $state(false);
  let selected = $state(null);
  let relatedParent = $state(null);
  let relatedChildren = $state([]);
  let relatedLoading = $state(false);
  let killing = $state(false);
  let toastMessage = $state("");
  let toastVisible = $state(false);
  let scope = $state("local");
  let connectionId = $state(null);
  let sortKey = $state("memory");
  let sortDir = $state("desc");

  let toastTimer = 0;
  let searchTimer = 0;
  let relatedSeq = 0;

  const needle = $derived(query.trim().toLowerCase());
  const filtered = $derived(
    sortItems(
      items.filter((item) => processMatches(item, needle)),
      sortKey,
      sortDir,
    ),
  );
  const hasFilter = $derived(Boolean(needle));

  function sortValue(item, key) {
    switch (key) {
      case "pid":
        return Number(item.pid) || 0;
      case "name":
        return String(item.name || "").toLowerCase();
      case "ports": {
        const ports = Array.isArray(item.ports) ? item.ports : [];
        if (ports.length === 0) return -1;
        return Math.min(...ports.map((socket) => Number(socket.port) || 0));
      }
      case "threads":
        return Number(item.threads) || 0;
      case "user":
        return String(item.user || "").toLowerCase();
      case "cpu":
        return Number(item.cpu) || 0;
      case "memory":
        return Number(item.memoryBytes) || 0;
      default:
        return 0;
    }
  }

  function sortItems(list, key, dir) {
    const factor = dir === "asc" ? 1 : -1;
    return [...list].sort((a, b) => {
      const left = sortValue(a, key);
      const right = sortValue(b, key);
      if (typeof left === "string" || typeof right === "string") {
        const cmp = String(left).localeCompare(String(right), undefined, {
          numeric: true,
          sensitivity: "base",
        });
        if (cmp !== 0) return cmp * factor;
      } else if (left !== right) {
        return (left - right) * factor;
      }
      return (Number(a.pid) || 0) - (Number(b.pid) || 0);
    });
  }

  function toggleSort(key) {
    if (sortKey === key) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortKey = key;
      sortDir =
        key === "name" || key === "user" || key === "pid" ? "asc" : "desc";
    }
  }

  function sortIndicator(key) {
    if (sortKey !== key) return "";
    return sortDir === "asc" ? " ↑" : " ↓";
  }

  function onSearchInput() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      loadAll().catch((error) => {
        showToast(`${text.error}: ${error.message}`);
      });
    }, 280);
  }

  function applyLocale() {
    text = resolveMessages(window.dbxPlugin?.locale || navigator.language || "en");
  }

  function showToast(message) {
    toastMessage = message;
    toastVisible = true;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastVisible = false;
    }, 1800);
  }

  function applyContext(context = {}) {
    connectionId = context.connectionId || context.connection?.id || null;
    if (connectionId) {
      scope = "remote";
    } else if (String(context.scope || "").toLowerCase() === "remote") {
      scope = "remote";
      connectionId = null;
    } else {
      scope = "local";
      connectionId = null;
    }
  }

  function rpcParams(extra = {}) {
    if (scope === "remote") {
      if (!connectionId) {
        throw new Error(text.remotePending);
      }
      return { ...extra, scope: "remote", connectionId };
    }
    return { ...extra, scope: "local" };
  }

  async function loadAll() {
    const [listResult, summaryResult] = await Promise.all([
      window.dbxPlugin.invoke("process/list", rpcParams({ query: query.trim(), limit: 500 })),
      window.dbxPlugin.invoke("process/summary", rpcParams()),
    ]);
    items = Array.isArray(listResult?.items) ? listResult.items : [];
    summary = summaryResult || null;
    if (listResult?.scope) scope = listResult.scope;
    if (selected) {
      const next = items.find((item) => item.pid === selected.pid) || selected;
      selected = next;
      await loadRelated(next.pid);
    }
  }

  async function loadRelated(pid) {
    const seq = ++relatedSeq;
    relatedLoading = true;
    relatedParent = null;
    relatedChildren = [];
    try {
      const result = await window.dbxPlugin.invoke(
        "process/related",
        rpcParams({ pid, childLimit: 100 }),
      );
      if (seq !== relatedSeq) return;
      relatedParent = result?.parent || null;
      relatedChildren = Array.isArray(result?.children) ? result.children : [];
    } catch {
      if (seq !== relatedSeq) return;
      relatedParent = null;
      relatedChildren = [];
    } finally {
      if (seq === relatedSeq) relatedLoading = false;
    }
  }

  function openProcess(item) {
    if (!item) return;
    selected = item;
    void loadRelated(item.pid);
  }

  async function revealPath(path) {
    if (!path) return;
    try {
      await window.dbxPlugin.invoke("process/reveal-path", { path });
      showToast(text.revealed);
    } catch (error) {
      showToast(`${text.error}: ${error.message}`);
    }
  }

  async function refresh() {
    refreshing = true;
    try {
      await loadAll();
      showToast(text.refreshed);
    } catch (error) {
      showToast(`${text.error}: ${error.message}`);
    } finally {
      refreshing = false;
    }
  }

  async function performKill(signal) {
    if (!selected) return;
    const pid = selected.pid;
    const name = selected.name || text.untitled;
    killing = true;
    let succeeded = false;
    try {
      const result = await window.dbxPlugin.invoke(
        "process/kill",
        rpcParams({ pid, signal }),
      );
      if (result && result.success === false) {
        throw new Error(result.message || result.error || text.error);
      }
      succeeded = true;
      selected = null;
      relatedParent = null;
      relatedChildren = [];
    } catch (error) {
      showToast(`${text.error}: ${error.message}`);
      // Keep the detail modal open on failure.
    } finally {
      killing = false;
    }
    // Refresh after releasing the kill lock — list refresh must not freeze the modal.
    if (succeeded) {
      let stillAlive = false;
      try {
        // Confirm by PID so the current search filter cannot hide a still-living process.
        // limit:0 keeps all PID substring matches so a short PID can't fall out of top-N.
        const check = await window.dbxPlugin.invoke(
          "process/list",
          rpcParams({ query: String(pid), limit: 0 }),
        );
        stillAlive = (Array.isArray(check?.items) ? check.items : []).some(
          (item) => Number(item.pid) === Number(pid),
        );
      } catch {
        // Verification failed — still treat signal success as terminated for toast.
      }
      showToast(stillAlive ? text.killSignalSent(name) : text.killed(name));
      try {
        await loadAll();
      } catch (error) {
        showToast(`${text.error}: ${error.message}`);
      }
    }
  }

  let unsubscribeContext = null;

  onMount(() => {
    window.dbxPlugin.ready.then(async () => {
      applyLocale();
      window.addEventListener("dbx-plugin-env", applyLocale);

      applyContext(window.dbxPlugin.context || {});
      if (typeof window.dbxPlugin.onContext === "function") {
        unsubscribeContext = window.dbxPlugin.onContext((next) => {
          const previousId = connectionId;
          const previousScope = scope;
          applyContext(next || {});
          if (previousId === connectionId && previousScope === scope) return;
          bootError = "";
          selected = null;
          relatedParent = null;
          relatedChildren = [];
          loadAll().catch((error) => {
            bootError = `${text.error}: ${error.message || text.remotePending}`;
          });
        });
      }

      try {
        await loadAll();
      } catch (error) {
        bootError = `${text.error}: ${error.message || text.remotePending}`;
      }
    });
  });

  onDestroy(() => {
    clearTimeout(toastTimer);
    clearTimeout(searchTimer);
    window.removeEventListener("dbx-plugin-env", applyLocale);
    if (typeof unsubscribeContext === "function") unsubscribeContext();
  });
</script>

<svelte:head>
  <title>{text.title}</title>
</svelte:head>

<div class="app">
  <div class="toolbar">
    <span class="badge" class:remote={scope === "remote"}>
      {scope === "remote" ? text.scopeRemote : text.scopeLocal}
    </span>
    <input
      class="search"
      type="search"
      autocomplete="off"
      spellcheck="false"
      placeholder={text.search}
      bind:value={query}
      oninput={onSearchInput}
    />
    <button
      class="icon-btn"
      type="button"
      title={text.refresh}
      aria-label={text.refresh}
      disabled={refreshing}
      onclick={refresh}
    >
      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
        <path
          fill="currentColor"
          d="M17.65 6.35A7.95 7.95 0 0 0 12 4a8 8 0 1 0 7.75 10h-2.1A6 6 0 1 1 12 6c1.57 0 2.98.61 4.04 1.61L13 10h7V3z"
        />
      </svg>
    </button>
  </div>

  <div class="meta">
    <span>
      {text.count(filtered.length)}
      {#if summary}
        · {text.memory(formatBytes(summary.usedBytes), formatBytes(summary.totalBytes))}
      {/if}
      · {text.hint}
    </span>
  </div>

  <div class="table-wrap">
    {#if bootError}
      <div class="error">{bootError}</div>
    {:else if filtered.length === 0}
      <div class="empty">{hasFilter ? text.emptyFilter : text.empty}</div>
    {:else}
      <table>
        <thead>
          <tr>
            <th>
              <button
                class="sort-btn"
                class:active={sortKey === "pid"}
                type="button"
                onclick={() => toggleSort("pid")}
              >
                {text.colPid}{sortIndicator("pid")}
              </button>
            </th>
            <th>
              <button
                class="sort-btn"
                class:active={sortKey === "name"}
                type="button"
                onclick={() => toggleSort("name")}
              >
                {text.colName}{sortIndicator("name")}
              </button>
            </th>
            <th>
              <button
                class="sort-btn"
                class:active={sortKey === "ports"}
                type="button"
                onclick={() => toggleSort("ports")}
              >
                {text.colPorts}{sortIndicator("ports")}
              </button>
            </th>
            <th class="num">
              <button
                class="sort-btn num"
                class:active={sortKey === "threads"}
                type="button"
                onclick={() => toggleSort("threads")}
              >
                {text.colThreads}{sortIndicator("threads")}
              </button>
            </th>
            <th>
              <button
                class="sort-btn"
                class:active={sortKey === "user"}
                type="button"
                onclick={() => toggleSort("user")}
              >
                {text.colUser}{sortIndicator("user")}
              </button>
            </th>
            <th class="num">
              <button
                class="sort-btn num"
                class:active={sortKey === "cpu"}
                type="button"
                onclick={() => toggleSort("cpu")}
              >
                {text.colCpu}{sortIndicator("cpu")}
              </button>
            </th>
            <th class="num">
              <button
                class="sort-btn num"
                class:active={sortKey === "memory"}
                type="button"
                onclick={() => toggleSort("memory")}
              >
                {text.colMem}{sortIndicator("memory")}
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as item (item.pid)}
            <tr
              class="clickable"
              class:selected={selected?.pid === item.pid}
              title={item.cmd || item.name}
              tabindex="0"
              onclick={() => openProcess(item)}
              onkeydown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  openProcess(item);
                }
              }}
            >
              <td class="mono">{item.pid}</td>
              <td class="name">
                <span class="name-cell">
                  <ProcessIcon iconKey={item.iconKey || null} name={item.name || ""} />
                  <span class="name-text">{item.name || "—"}</span>
                </span>
              </td>
              <td class="mono ports">
                {#each [normalizePorts(item.ports)] as ports}
                  {#if ports.length === 0}
                    —
                  {:else}
                    {#each ports as socket, index (`${socket.protocol}:${socket.port}:${index}`)}
                      {#if index > 0}<span class="port-sep">, </span>{/if}
                      <span
                        class="port"
                        class:tcp={socket.protocol === "tcp"}
                        class:udp={socket.protocol === "udp"}
                        title={socket.protocol.toUpperCase()}
                      >{socket.port}</span>
                    {/each}
                  {/if}
                {/each}
              </td>
              <td class="num mono">{item.threads ?? 0}</td>
              <td>{item.user || "—"}</td>
              <td class="num mono">{formatCpu(item.cpu)}</td>
              <td class="num mono">{formatBytes(item.memoryBytes)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<Toast message={toastMessage} visible={toastVisible} />

<ProcessDetailModal
  open={Boolean(selected)}
  item={selected}
  {text}
  confirming={killing}
  remote={scope === "remote"}
  parent={relatedParent}
  childProcesses={relatedChildren}
  {relatedLoading}
  onclose={() => {
    selected = null;
    relatedParent = null;
    relatedChildren = [];
  }}
  onkill={() => performKill("TERM")}
  onselect={openProcess}
  onreveal={revealPath}
/>

<style>
  :global(:root) {
    color-scheme: light dark;
    --bg: var(--color-background, Canvas);
    --fg: var(--color-foreground, CanvasText);
    --muted: color-mix(in srgb, var(--fg) 55%, transparent);
    --border: color-mix(in srgb, var(--fg) 14%, transparent);
    --hover: color-mix(in srgb, var(--fg) 6%, transparent);
    --active: color-mix(in srgb, var(--fg) 10%, transparent);
    --accent: var(--color-primary, #2563eb);
    --danger: #dc2626;
    --radius: var(--radius-md, 10px);
    --font: var(--font-sans, ui-sans-serif, system-ui, -apple-system, sans-serif);
    --mono: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: var(--bg);
    color: var(--fg);
    font: 13px/1.45 var(--font);
  }

  :global(#app) {
    height: 100%;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .badge {
    flex-shrink: 0;
    height: 26px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    display: grid;
    place-items: center;
    font-size: 12px;
    color: var(--muted);
  }

  .badge.remote {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }

  .search {
    flex: 1;
    min-width: 0;
    height: 34px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: transparent;
    color: inherit;
    font: inherit;
    outline: none;
  }

  .search:focus {
    border-color: color-mix(in srgb, var(--accent) 70%, var(--border));
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .icon-btn {
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: transparent;
    color: inherit;
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-btn:hover {
    background: var(--hover);
  }

  .icon-btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .meta {
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
    flex-shrink: 0;
  }

  .table-wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: middle;
  }

  th {
    position: sticky;
    top: 0;
    background: var(--bg);
    color: var(--muted);
    font-weight: 600;
    font-size: 12px;
    z-index: 1;
    padding: 4px 6px;
  }

  .sort-btn {
    display: inline-flex;
    align-items: center;
    width: 100%;
    margin: 0;
    padding: 4px 6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-weight: 600;
    font-size: 12px;
    cursor: pointer;
    white-space: nowrap;
  }

  .sort-btn.num {
    justify-content: flex-end;
  }

  .sort-btn:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .sort-btn.active {
    color: var(--fg);
  }

  tr.clickable {
    cursor: pointer;
  }

  tr.clickable:hover td {
    background: var(--hover);
  }

  tr.clickable.selected td {
    background: var(--active);
  }

  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }

  .ports {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .port-sep {
    color: var(--muted);
  }

  .port.tcp {
    color: light-dark(#2563eb, #60a5fa);
  }

  .port.udp {
    color: light-dark(#d97706, #fbbf24);
  }

  .name {
    max-width: 260px;
  }

  .name-cell {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    min-width: 0;
  }

  .name-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .empty,
  .error {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 32px;
    text-align: center;
    color: var(--muted);
    white-space: pre-line;
  }

  .error {
    color: var(--danger);
  }
</style>
