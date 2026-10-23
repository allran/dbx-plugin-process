/** Fallback client-side guard when older backends omit `protected`. */
export function isProtectedProcess(item) {
  if (!item) return false;
  if (item.protected) return true;

  const pid = Number(item.pid);
  if (pid === 0 || pid === 1 || pid === 4) return true;

  const raw = String(item.name || "").trim();
  if (!raw) return false;
  if (raw.startsWith("[") && raw.endsWith("]")) return true;

  const base = raw
    .split(/[/\\]/)
    .pop()
    .replace(/\.exe$/i, "")
    .toLowerCase();

  return [
    "launchd",
    "kernel_task",
    "init",
    "systemd",
    "kthreadd",
    "system",
    "registry",
    "smss",
    "csrss",
    "wininit",
    "services",
    "lsass",
    "winlogon",
    "fontdrvhost",
  ].includes(base);
}
