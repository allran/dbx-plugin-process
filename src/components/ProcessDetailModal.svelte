<script>
  import { formatBytes, formatCpu, normalizePorts } from "../lib/format.js";
  import { isProtectedProcess } from "../lib/protected.js";
  import ProcessIcon from "./ProcessIcon.svelte";

  let {
    open = false,
    item = null,
    text,
    confirming = false,
    remote = false,
    parent = null,
    childProcesses = [],
    relatedLoading = false,
    onclose,
    onkill,
    onselect,
    onreveal,
  } = $props();

  let pendingKill = $state(false);

  const protectedProcess = $derived(isProtectedProcess(item));
  const ports = $derived(normalizePorts(item?.ports));
  const tcpPorts = $derived(
    ports.filter((socket) => socket.protocol === "tcp").map((socket) => socket.port),
  );
  const udpPorts = $derived(
    ports.filter((socket) => socket.protocol === "udp").map((socket) => socket.port),
  );
  const processLabel = $derived(item?.name || text.untitled);

  $effect(() => {
    // Reset confirm step when the modal closes or the selected process changes.
    if (!open || !item) {
      pendingKill = false;
      return;
    }
    item.pid;
    pendingKill = false;
  });

  function requestKill() {
    if (confirming || protectedProcess) return;
    pendingKill = true;
  }

  function cancelKill() {
    if (confirming) return;
    pendingKill = false;
  }

  function confirmKill() {
    if (!pendingKill || confirming || protectedProcess) return;
    onkill?.();
  }

  function closeModal() {
    if (confirming) return;
    if (pendingKill) {
      pendingKill = false;
      return;
    }
    onclose?.();
  }

  function onKeydown(event) {
    if (event.key === "Escape" && open) closeModal();
  }

  function formatStartTime(seconds) {
    if (!seconds) return "—";
    try {
      return new Date(Number(seconds) * 1000).toLocaleString();
    } catch {
      return String(seconds);
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open && item}
  <div class="modal">
    <button class="modal-backdrop" type="button" aria-label={text.close} onclick={closeModal}
    ></button>
    <div class="modal-panel" role="dialog" aria-modal="true" aria-labelledby="detail-title">
      <div class="header">
        {#key `${item.pid}:${item.iconKey || ""}`}
          <ProcessIcon iconKey={item.iconKey || null} name={item.name || ""} size={40} />
        {/key}
        <div class="header-text">
          <h2 id="detail-title" class="modal-title">{item.name || text.untitled}</h2>
          <div class="subtitle mono">PID {item.pid}</div>
        </div>
        <button class="close-btn" type="button" aria-label={text.close} onclick={closeModal}>
          ×
        </button>
      </div>

      <div class="modal-body">
        <dl class="grid">
          <div class="row">
            <dt>{text.colUser}</dt>
            <dd>{item.user || "—"}</dd>
          </div>
          <div class="row">
            <dt>{text.colCpu}</dt>
            <dd class="mono">{formatCpu(item.cpu)}</dd>
          </div>
          <div class="row">
            <dt>{text.colMem}</dt>
            <dd class="mono">{formatBytes(item.memoryBytes)}</dd>
          </div>
          <div class="row">
            <dt>{text.colThreads}</dt>
            <dd class="mono">{item.threads ?? 0}</dd>
          </div>
          <div class="row">
            <dt>{text.colStarted}</dt>
            <dd>{formatStartTime(item.startTime)}</dd>
          </div>
          <div class="row">
            <dt>{text.colStatus}</dt>
            <dd>{item.status || "—"}</dd>
          </div>
          <div class="row wide">
            <dt>{text.colPorts}</dt>
            <dd class="mono ports">
              {#if ports.length === 0}
                —
              {:else}
                {#if tcpPorts.length > 0}
                  <div class="port-line tcp">
                    <span class="port-proto">TCP:</span>
                    <span class="port-list">{tcpPorts.join(", ")}</span>
                  </div>
                {/if}
                {#if udpPorts.length > 0}
                  <div class="port-line udp">
                    <span class="port-proto">UDP:</span>
                    <span class="port-list">{udpPorts.join(", ")}</span>
                  </div>
                {/if}
              {/if}
            </dd>
          </div>
          <div class="row wide">
            <dt class="dt-with-action">
              <span>{text.colExe}</span>
              {#if item.exe && !remote}
                <button
                  class="icon-action"
                  type="button"
                  title={text.revealInFolder}
                  aria-label={text.revealInFolder}
                  onclick={() => onreveal?.(item.exe)}
                >
                  <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                    <path
                      fill="currentColor"
                      d="M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2zm0 2 2 2h8v10H4V6h6z"
                    />
                  </svg>
                </button>
              {/if}
            </dt>
            <dd class="wrap mono">{item.exe || "—"}</dd>
          </div>
          <div class="row wide">
            <dt>{text.colCwd}</dt>
            <dd class="wrap mono">{item.cwd || "—"}</dd>
          </div>
          <div class="row wide">
            <dt>{text.colCmd}</dt>
            <dd class="wrap mono">{item.cmd || "—"}</dd>
          </div>
        </dl>

        <section class="relation">
          <h3 class="section-title">{text.colParent}</h3>
          {#if relatedLoading}
            <div class="relation-empty">…</div>
          {:else if parent}
            <button class="relation-item" type="button" onclick={() => onselect?.(parent)}>
              <ProcessIcon iconKey={parent.iconKey || null} name={parent.name || ""} size={22} />
              <span class="relation-body">
                <span class="relation-name">{parent.name || text.untitled}</span>
                <span class="relation-meta mono">
                  PID {parent.pid} · {formatBytes(parent.memoryBytes)} · {formatCpu(parent.cpu)}
                </span>
              </span>
            </button>
          {:else if item.parentPid}
            <div class="relation-empty">{text.parentPidOnly(item.parentPid)}</div>
          {:else}
            <div class="relation-empty">{text.noParent}</div>
          {/if}
        </section>

        <section class="relation">
          <h3 class="section-title">
            {text.colChildren}
            {#if !relatedLoading && childProcesses.length > 0}
              <span class="section-count">{text.childrenCount(childProcesses.length)}</span>
            {/if}
          </h3>
          {#if relatedLoading}
            <div class="relation-empty">…</div>
          {:else if childProcesses.length === 0}
            <div class="relation-empty">{text.noChildren}</div>
          {:else}
            <div class="relation-list">
              {#each childProcesses as child (child.pid)}
                <button class="relation-item" type="button" onclick={() => onselect?.(child)}>
                  <ProcessIcon iconKey={child.iconKey || null} name={child.name || ""} size={22} />
                  <span class="relation-body">
                    <span class="relation-name">{child.name || text.untitled}</span>
                    <span class="relation-meta mono">
                      PID {child.pid} · {formatBytes(child.memoryBytes)} · {formatCpu(child.cpu)}
                    </span>
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </section>
      </div>

      <div class="modal-actions">
        {#if protectedProcess}
          <div class="protected-hint">{text.killProtected}</div>
          <button class="btn" type="button" disabled={confirming} onclick={closeModal}>
            {text.close}
          </button>
        {:else if pendingKill}
          <div class="confirm-hint">{text.killConfirmAsk(processLabel, item.pid)}</div>
          <button class="btn" type="button" disabled={confirming} onclick={cancelKill}>
            {text.cancel}
          </button>
          <button
            class="btn btn-primary"
            type="button"
            disabled={confirming}
            onclick={confirmKill}
          >
            {text.killConfirmYes}
          </button>
        {:else}
          <button
            class="btn btn-primary"
            type="button"
            disabled={confirming}
            title={text.killConfirm}
            onclick={requestKill}
          >
            {text.killConfirm}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .modal {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 20px;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: color-mix(in srgb, #000 45%, transparent);
    cursor: pointer;
  }

  .modal-panel {
    position: relative;
    display: flex;
    flex-direction: column;
    width: min(560px, 100%);
    max-height: min(84vh, 720px);
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--bg);
    box-shadow: 0 18px 50px color-mix(in srgb, #000 28%, transparent);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
    padding: 16px 20px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }

  .modal-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 14px 20px;
  }

  .header-text {
    min-width: 0;
    flex: 1;
  }

  .modal-title {
    margin: 0;
    font-size: 16px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subtitle {
    color: var(--muted);
    font-size: 12px;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
  }

  .close-btn:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .grid {
    margin: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px 16px;
  }

  .row {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .row.wide {
    grid-column: 1 / -1;
  }

  dt {
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .dt-with-action {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .icon-action {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    padding: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-action:hover {
    background: var(--hover);
    color: var(--accent);
  }

  dd {
    margin: 0;
    word-break: break-word;
  }

  .wrap {
    white-space: pre-wrap;
  }

  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }

  .ports {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .port-line {
    display: flex;
    gap: 6px;
    align-items: baseline;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .port-proto {
    flex-shrink: 0;
    font-weight: 650;
  }

  .port-line.tcp {
    color: light-dark(#2563eb, #60a5fa);
  }

  .port-line.udp {
    color: light-dark(#d97706, #fbbf24);
  }

  .relation {
    margin-top: 16px;
  }

  .section-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 650;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .section-count {
    font-weight: 500;
    text-transform: none;
    letter-spacing: 0;
    color: var(--fg);
    opacity: 0.7;
  }

  .relation-list {
    display: grid;
    gap: 6px;
  }

  .relation-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .relation-item:hover {
    background: var(--hover);
  }

  .relation-body {
    min-width: 0;
    display: grid;
    gap: 2px;
  }

  .relation-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .relation-meta {
    color: var(--muted);
  }

  .relation-empty {
    padding: 10px 12px;
    border: 1px dashed var(--border);
    border-radius: 10px;
    color: var(--muted);
    font-size: 12px;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    flex-shrink: 0;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }

  .protected-hint,
  .confirm-hint {
    flex: 1;
    min-width: 160px;
    margin-right: auto;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.4;
  }

  .confirm-hint {
    color: var(--fg);
  }

  .btn {
    height: 34px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }

  .btn:hover {
    background: var(--hover);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }

  .btn-primary {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }

  .btn-primary:hover {
    filter: brightness(0.95);
    background: var(--danger);
  }
</style>
