// Parses user input into a SocketAddr string Rust can parse.
// Accepted forms:
//   ""            -> "0.0.0.0:9600"  (empty -> default IPv4 + default port)
//   "1.2.3.4"     -> "1.2.3.4:9600"  (bare IPv4 -> default port)
//   "::1"         -> "[::1]:9600"    (bare IPv6 -> bracketed + default port)
//   "1.2.3.4:80"  -> "1.2.3.4:80"    (IPv4:port -> as-is)
//   "[::1]:80"    -> "[::1]:80"      (bracketed IPv6:port -> as-is)
// Returns null if the input cannot be resolved to a valid address.
export function parseSocketAddress(
    input: string,
    defaultIp: string,
    defaultPort: number,
): string | null {
    const trimmed = input.trim();

    if (trimmed === "") return `${defaultIp}:${defaultPort}`;

    // Bracketed IPv6: [addr] or [addr]:port
    if (trimmed.startsWith("[")) {
        const closeBracket = trimmed.indexOf("]");
        if (closeBracket === -1) return null;
        const ip = trimmed.slice(1, closeBracket);
        if (!isValidIpv6(ip)) return null;
        const rest = trimmed.slice(closeBracket + 1);
        if (rest === "") return `[${ip}]:${defaultPort}`;
        if (!rest.startsWith(":")) return null;
        const port = parsePort(rest.slice(1));
        return port !== null ? `[${ip}]:${port}` : null;
    }

    // Bare IPv6: more than one colon means it can't be IPv4:port
    const colonCount = (trimmed.match(/:/g) ?? []).length;
    if (colonCount > 1) {
        if (!isValidIpv6(trimmed)) return null;
        return `[${trimmed}]:${defaultPort}`;
    }

    // IPv4 with optional :port
    const colonIdx = trimmed.indexOf(":");
    if (colonIdx === -1) {
        return isValidIpv4(trimmed) ? `${trimmed}:${defaultPort}` : null;
    }

    const ip = trimmed.slice(0, colonIdx);
    const port = parsePort(trimmed.slice(colonIdx + 1));
    return isValidIpv4(ip) && port !== null ? `${ip}:${port}` : null;
}

function parsePort(raw: string): number | null {
    const port = Number(raw);
    return Number.isInteger(port) && port >= 1 && port <= 65535 && raw === String(port)
        ? port
        : null;
}

function isValidIpv4(ip: string): boolean {
    const parts = ip.split(".");
    if (parts.length !== 4) return false;
    return parts.every(part => {
        const num = Number(part);
        return Number.isInteger(num) && num >= 0 && num <= 255 && part === String(num);
    });
}

function isValidIpv6(ip: string): boolean {
    if (ip === "::") return true;

    const sides = ip.split("::");
    if (sides.length > 2) return false; // multiple :: not allowed

    const isHexGroup = (s: string) => /^[0-9a-fA-F]{1,4}$/.test(s);
    const toGroups = (s: string) => (s === "" ? [] : s.split(":"));

    if (sides.length === 2) {
        const groups = [...toGroups(sides[0]), ...toGroups(sides[1])];
        return groups.length < 8 && groups.every(isHexGroup);
    }

    // No ::, must be exactly 8 groups
    const groups = ip.split(":");
    return groups.length === 8 && groups.every(isHexGroup);
}
