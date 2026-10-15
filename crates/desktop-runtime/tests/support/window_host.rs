use uuid::Uuid;
use zmux_desktop_runtime::{
    Desktop,
    windows::{NativeWindowEvent, WindowHostToken, WindowKind},
};
/// Establishes the same native host/key observations required by production;
/// app activation alone is deliberately insufficient to consume attention.
pub fn observe_key(desktop: &Desktop) {
    let snapshot = desktop.snapshot().unwrap();
    let window = snapshot.model.focused_window.unwrap();
    let token = if let Some(token) = snapshot
        .window_hosts
        .iter()
        .find(|token| token.window_id == window)
    {
        token.clone()
    } else {
        let request = snapshot
            .window_requests
            .iter()
            .find(|request| request.window_id == window && request.kind == WindowKind::Open)
            .unwrap();
        let token = WindowHostToken {
            epoch: request.epoch,
            identity_namespace: request.identity_namespace,
            window_id: window,
            host_id: Uuid::new_v4(),
        };
        desktop
            .native_window_event(NativeWindowEvent::Complete {
                token: token.clone(),
                operation_id: request.operation_id,
            })
            .unwrap();
        token
    };
    desktop
        .native_window_event(NativeWindowEvent::Key { token, key: true })
        .unwrap();
}
