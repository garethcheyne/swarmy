//! Live updates for the UI: pushes the latest snapshot every time it changes
//! (once a second).

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};

use crate::state::Shared;

pub async fn handler(State(state): State<Shared>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| stream(state, socket))
}

async fn stream(state: Shared, mut socket: WebSocket) {
    let mut rx = state.snapshot.clone();
    loop {
        let json = {
            let snap = rx.borrow_and_update();
            match serde_json::to_string(&**snap) {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!("serializing snapshot: {e}");
                    return;
                }
            }
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
        tokio::select! {
            changed = rx.changed() => if changed.is_err() { return },
            msg = socket.recv() => match msg {
                // Ignore client messages; stop on close/error.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
