import WebSocket from "ws";

const SERVER_BASE = "http://127.0.0.1:4568";
const WS_URL = "ws://127.0.0.1:4568/ws";
const PROTOCOL_VERSION = "2.0.0";

export type ServerMessage = {
    type: string;
    joinedParticipants?: Record<string, unknown>;
    [key: string]: unknown;
};

/**
 * A lightweight signaling client speaking the vacs WebSocket protocol
 * directly. Used as additional call participants beyond the two real app
 * instances (e.g. to fill the incoming call queue).
 */
export class SignalingTestClient {
    readonly cid: string;
    private ws: WebSocket;
    private messages: ServerMessage[] = [];
    private waiters: {check: () => boolean; resolve: () => void}[] = [];
    private listeners = new Set<(msg: ServerMessage) => void>();

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
            // Listeners run inside the receive path on purpose: a reply sent
            // from one is on the wire before anything the test does next.
            for (const listener of client.listeners) {
                listener(msg);
            }
            client.waiters = client.waiters.filter(waiter => {
                if (waiter.check()) {
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
            targets: [{client: targetCid}],
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

    /** Sends a rejection for a call this client was invited to. */
    reject(callId: string, reason: string = "busy"): void {
        this.send({type: "callReject", callId, rejectingClientId: this.cid, reason});
    }

    /**
     * Registers a listener called synchronously for every received message,
     * from inside the WebSocket receive path. Returns a function removing it.
     */
    onMessage(listener: (msg: ServerMessage) => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    /**
     * Rejects every call invitation as busy, straight from the receive path.
     * That is what makes the rejection beat the caller's own IPC reply to the
     * invite it just sent. Returns a function stopping the auto-rejection.
     */
    autoRejectInvitations(): () => void {
        return this.onMessage(msg => {
            if (msg.type === "callInvitation") {
                this.reject(msg.callId as string);
            }
        });
    }

    /** Resolves once any received message matches the predicate. */
    async waitForMessage(
        predicate: (msg: ServerMessage) => boolean,
        timeoutMs: number = 5000,
    ): Promise<ServerMessage> {
        const [message] = await this.waitForMessages(predicate, 1, timeoutMs);
        return message;
    }

    /**
     * Resolves once at least `count` received messages match the predicate,
     * with the matches in arrival order. Messages received before the call
     * count, so a burst cannot be missed by waiting too late.
     */
    async waitForMessages(
        predicate: (msg: ServerMessage) => boolean,
        count: number,
        timeoutMs: number = 5000,
    ): Promise<ServerMessage[]> {
        const matches = () => this.messages.filter(predicate);
        const check = () => matches().length >= count;

        if (!check()) {
            await new Promise<void>((resolve, reject) => {
                const waiter = {
                    check,
                    resolve: () => {
                        clearTimeout(timer);
                        resolve();
                    },
                };
                const timer = setTimeout(() => {
                    this.waiters = this.waiters.filter(entry => entry !== waiter);
                    reject(
                        new Error(
                            `Timed out waiting for ${count} signaling message(s), ` +
                                `saw ${matches().length}`,
                        ),
                    );
                }, timeoutMs);
                this.waiters.push(waiter);
            });
        }

        return matches().slice(0, count);
    }

    disconnect(): void {
        this.ws.close();
    }
}
