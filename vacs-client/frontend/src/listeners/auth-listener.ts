import {useAuthStore} from "../stores/auth-store.ts";
import {listen, UnlistenFn} from "../transport";
import {ClientId} from "../types/generic.ts";

export function setupAuthListeners() {
    const {setAuthenticated, setUnauthenticated} = useAuthStore.getState();

    const unlistenFns: Promise<UnlistenFn>[] = [];

    const init = () => {
        unlistenFns.push(
            listen<ClientId>("auth:authenticated", event => {
                setAuthenticated(event.payload);
            }),
            listen("auth:unauthenticated", () => {
                setUnauthenticated();
            }),
        );
    };

    init();

    return () => {
        unlistenFns.forEach(fn => fn.then(f => f()));
    };
}
