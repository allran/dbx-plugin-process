export function formatBytes(bytes) {
  const value = Number(bytes) || 0;
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let n = value;
  let i = -1;
  do {
    n /= 1024;
    i += 1;
  } while (n >= 1024 && i < units.length - 1);
  return `${n.toFixed(n >= 100 ? 0 : n >= 10 ? 1 : 2)} ${units[i]}`;
}

export function formatCpu(cpu) {
  const value = Number(cpu) || 0;
  return `${value.toFixed(1)}%`;
}

export function normalizePorts(ports) {
  if (!Array.isArray(ports)) return [];
  return ports
    .map((entry) => {
      if (entry && typeof entry === "object") {
        const protocol = String(entry.protocol || "").toLowerCase();
        const port = Number(entry.port);
        if ((protocol === "tcp" || protocol === "udp") && port > 0) {
          return { protocol, port };
        }
        return null;
      }
      // Backward-compatible bare port numbers.
      const port = Number(entry);
      return port > 0 ? { protocol: "tcp", port } : null;
    })
    .filter(Boolean);
}

function parsePortQuery(needle) {
  const raw = String(needle || "")
    .trim()
    .toLowerCase();
  if (!raw) return null;

  let protocol = null;
  let rest = raw;
  if (raw.startsWith("tcp:") || raw.startsWith("tcp ")) {
    protocol = "tcp";
    rest = raw.slice(4).trim();
  } else if (raw.startsWith("udp:") || raw.startsWith("udp ")) {
    protocol = "udp";
    rest = raw.slice(4).trim();
  } else if (raw.startsWith("port:") || raw.startsWith("port ")) {
    rest = raw.slice(5).trim();
  } else if (raw.startsWith(":")) {
    rest = raw.slice(1).trim();
  }

  if (!/^\d+$/.test(rest)) return null;
  const port = Number(rest);
  if (!(port > 0 && port <= 65535)) return null;
  return { port, protocol };
}

function socketMatches(socket, query) {
  if (socket.port !== query.port) return false;
  if (!query.protocol) return true;
  return socket.protocol === query.protocol;
}

export function processMatches(item, needle) {
  if (!needle) return true;
  const ports = normalizePorts(item.ports);
  const portOnly =
    needle.startsWith(":") ||
    needle.startsWith("port:") ||
    needle.startsWith("port ") ||
    needle.startsWith("tcp:") ||
    needle.startsWith("tcp ") ||
    needle.startsWith("udp:") ||
    needle.startsWith("udp ");
  const portQuery = parsePortQuery(needle);

  if (portOnly) {
    return portQuery != null && ports.some((socket) => socketMatches(socket, portQuery));
  }
  if (portQuery != null && ports.some((socket) => socketMatches(socket, portQuery))) {
    return true;
  }

  return (
    String(item.pid).includes(needle) ||
    String(item.name || "")
      .toLowerCase()
      .includes(needle) ||
    ports.some(
      (socket) =>
        String(socket.port).includes(needle) ||
        `${socket.protocol} ${socket.port}`.includes(needle) ||
        `${socket.protocol}:${socket.port}`.includes(needle),
    )
  );
}
