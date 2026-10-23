/** @type {Map<string, string | null>} */
const cache = new Map();
/** @type {Map<string, Promise<string | null>>} */
const inflight = new Map();

/** Keep sidecar free for kill/list — icon RPC is single-threaded. */
const MAX_CONCURRENT = 2;
let active = 0;
/** @type {Array<() => void>} */
const waiters = [];

function pumpQueue() {
  while (active < MAX_CONCURRENT && waiters.length > 0) {
    const next = waiters.shift();
    if (next) next();
  }
}

/**
 * @template T
 * @param {() => Promise<T>} task
 * @returns {Promise<T>}
 */
function enqueue(task) {
  return new Promise((resolve, reject) => {
    const run = () => {
      active += 1;
      task()
        .then(resolve, reject)
        .finally(() => {
          active -= 1;
          pumpQueue();
        });
    };
    waiters.push(run);
    pumpQueue();
  });
}

/**
 * @param {string | null | undefined} key
 * @returns {Promise<string | null>}
 */
export function loadProcessIcon(key) {
  if (!key) return Promise.resolve(null);
  if (cache.has(key)) return Promise.resolve(cache.get(key) ?? null);
  if (inflight.has(key)) return inflight.get(key);

  const request = enqueue(() =>
    window.dbxPlugin
      .invoke("process/icon", { key })
      .then((result) => {
        const dataUrl = result?.dataUrl || null;
        cache.set(key, dataUrl);
        return dataUrl;
      })
      .catch(() => {
        cache.set(key, null);
        return null;
      }),
  ).finally(() => {
    inflight.delete(key);
  });

  inflight.set(key, request);
  return request;
}

/**
 * @param {string | null | undefined} key
 * @returns {string | null | undefined} cached data URL if already loaded
 */
export function peekProcessIcon(key) {
  if (!key || !cache.has(key)) return undefined;
  return cache.get(key) ?? null;
}
