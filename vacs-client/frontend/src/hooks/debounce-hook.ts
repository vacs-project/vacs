import {useCallback, useEffect, useRef, useState} from "preact/hooks";

export function useAsyncDebounce<TArgs extends unknown[], TResult>(
    fn: (...args: TArgs) => Promise<TResult>,
): (...args: TArgs) => Promise<TResult | void> {
    const loadingRef = useRef<boolean>(false);

    return useCallback(
        async (...args: TArgs): Promise<TResult | void> => {
            if (loadingRef.current) return;
            loadingRef.current = true;
            try {
                return await fn(...args);
            } finally {
                loadingRef.current = false;
            }
        },
        [fn],
    );
}

export function useAsyncDebounceState<TArgs extends unknown[], TResult>(
    fn: (...args: TArgs) => Promise<TResult>,
): [(...args: TArgs) => Promise<TResult | void>, boolean] {
    const loadingRef = useRef(false);
    const [loading, setLoading] = useState<boolean>(false);
    const mountedRef = useRef(true);

    useEffect(
        () => () => {
            mountedRef.current = false;
        },
        [],
    );

    const wrapped = useCallback(
        async (...args: TArgs): Promise<TResult | void> => {
            if (loadingRef.current) return;
            loadingRef.current = true;
            if (mountedRef.current) setLoading(true);
            try {
                return await fn(...args);
            } finally {
                if (mountedRef.current) setLoading(false);
            }
        },
        [fn],
    );

    return [wrapped, loading];
}
