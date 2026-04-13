// Shim for @tauri-apps/api/event in web mode
// Provides no-op implementations for listen/emit since
// the web UI uses polling instead of Tauri events

type UnlistenFn = () => void;

export async function listen(
  _event: string,
  _handler: (event: any) => void
): Promise<UnlistenFn> {
  // In web mode, events are handled via polling the REST API
  // Return a no-op unlisten function
  return () => {};
}

export async function emit(_event: string, _payload?: any): Promise<void> {
  // No-op in web mode
}
