import * as React from "react";

export function useMountedRef(): React.MutableRefObject<boolean> {
  const mounted = React.useRef(false);
  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  return mounted;
}

export function markE2EProjectConnectionSettled(
  operation: "save" | "test" | "delete",
): void {
  if (import.meta.env.MODE !== "e2e") return;
  const counters = {
    save: "__BUZZ_E2E_PROJECT_CONNECTION_SAVE_UI_SETTLED__",
    test: "__BUZZ_E2E_PROJECT_CONNECTION_TEST_UI_SETTLED__",
    delete: "__BUZZ_E2E_PROJECT_CONNECTION_DELETE_UI_SETTLED__",
  } as const;
  const counter = counters[operation];
  window[counter] = (window[counter] ?? 0) + 1;
}
