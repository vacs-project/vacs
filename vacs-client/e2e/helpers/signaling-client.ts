import WebSocket from "ws";

const SERVER_BASE = "http://127.0.0.1:4568";
const WS_URL = "ws://127.0.0.1:4568/ws";
const PROTOCOL_VERSION = "2.0.0";

export type ServerMessage = {type: string} & Record<string, unknown>;

/**
 * A lightweight signaling client speaking the vacs WebSocket protocol
 * directly. Used as additional call participants beyond the two real app
 * instances (e.g. to fill the incoming call queue).
 */
export class SignalingTestClient {
    readonly cid: string;
    private ws: WebSocket;
    private messages: ServerMessage[] = [];
    private waiters: {predicate: (msg: ServerMessage) => boolean; resolve: () => void}[] = [];

    private constructor(cid: string, ws: WebSocket) {
        this.cid = cid;
        this.ws = ws;
    }

    /** Performs the full OAuth login and WebSocket handshake for the CID. */
    static async connect(cid: string): Promise<SignalingTestClient> {
        const cookies = new Map<string, string>();
        const storeCookies = (res: Response) => {
            for (const cookie of res.headers.getSetCookie()) {
                const [pair] = cookie.split(";");
                const idx = pair.indexOf("=");
                cookies.set(pair.slice(0, idx), pair.slice(idx + 1));
            }
        };
        const cookieHeader = () =>
            Array.from(cookies.entries())
                .map(([k, v]) => `${k}=${v}`)
                .join("; ");

        // Initiate the OAuth flow and follow the mock authorize redirect.
        const initResp = await fetch(`${SERVER_BASE}/auth/vatsim`);
        storeCookies(initResp);
        const {url} = (await initResp.json()) as {url: string};

        const authorizeUrl = new URL(url);
        authorizeUrl.searchParams.append("login_hint", cid);
        const authorizeResp = await fetch(authorizeUrl, {redirect: "manual"});
        const location = authorizeResp.headers.get("location");
        if (location === null) {
            throw new Error("Mock OAuth authorize did not redirect");
        }
        const redirectUrl = new URL(location);
        const code = redirectUrl.searchParams.get("code");
        const state = redirectUrl.searchParams.get("state");

        const callbackResp = await fetch(`${SERVER_BASE}/auth/vatsim/callback`, {
            method: "POST",
            headers: {"Content-Type": "application/json", Cookie: cookieHeader()},
            body: JSON.stringify({code, state}),
        });
        if (!callbackResp.ok) {
            throw new Error(`OAuth callback failed for ${cid}: ${callbackResp.status}`);
        }
        storeCookies(callbackResp);

        // Obtain a WebSocket auth token and log in.
        const tokenResp = await fetch(`${SERVER_BASE}/ws/token`, {
            headers: {Cookie: cookieHeader()},
        });
        if (!tokenResp.ok) {
            throw new Error(`Fetching ws token failed for ${cid}: ${tokenResp.status}`);
        }
        const {token} = (await tokenResp.json()) as {token: string};

        const ws = new WebSocket(WS_URL);
        const client = new SignalingTestClient(cid, ws);
        ws.on("message", data => {
            const msg = JSON.parse(data.toString()) as ServerMessage;
            client.messages.push(msg);
            client.waiters = client.waiters.filter(waiter => {
                if (client.messages.some(waiter.predicate)) {
                    waiter.resolve();
                    return false;
                }
                return true;
            });
        });
        await new Promise<void>((resolve, reject) => {
            ws.once("open", resolve);
            ws.once("error", reject);
        });

        client.send({
            type: "login",
            token,
            protocolVersion: PROTOCOL_VERSION,
            customProfile: false,
            positionId: null,
        });
        await client.waitForMessage(msg => msg.type === "sessionInfo");
        return client;
    }

    send(msg: Record<string, unknown>): void {
        this.ws.send(JSON.stringify(msg));
    }

    /** Sends a call invite to the given client and returns the call id. */
    invite(targetCid: string, options: {prio?: boolean} = {}): string {
        const callId = crypto.randomUUID();
        this.send({
            type: "callInvite",
            callId,
            source: {clientId: this.cid},
            target: {client: targetCid},
            prio: options.prio ?? false,
        });
        return callId;
    }

    accept(callId: string): void {
        this.send({type: "callAccept", callId, acceptingClientId: this.cid});
    }

    end(callId: string): void {
        this.send({type: "callEnd", callId, endingClientId: this.cid});
    }

    /** Resolves once any received message matches the predicate. */
    async waitForMessage(
        predicate: (msg: ServerMessage) => boolean,
        timeoutMs: number = 5000,
    ): Promise<ServerMessage> {
        const found = this.messages.find(predicate);
        if (found !== undefined) {
            return found;
        }
        await new Promise<void>((resolve, reject) => {
            const timer = setTimeout(
                () => reject(new Error("Timed out waiting for signaling message")),
                timeoutMs,
            );
            this.waiters.push({
                predicate,
                resolve: () => {
                    clearTimeout(timer);
                    resolve();
                },
            });
        });
        return this.messages.find(predicate) as ServerMessage;
    }

    disconnect(): void {
        this.ws.close();
    }
}
