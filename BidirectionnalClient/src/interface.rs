use axum::{
    extract::State,
    routing::{get, post},
    Json, Router, response::Html,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ClientInfo {
    pub id: u64,
    pub name: String,
}

#[derive(Clone)]
pub struct AppState {
    pub clients: Arc<Mutex<Vec<ClientInfo>>>,
    pub is_muted: Arc<Mutex<bool>>,
}

// Serve the HTML page
async fn index() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Rust Audio Chat</title>
    <style>
        body { font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; background-color: #121212; color: #e0e0e0; display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100vh; margin: 0; }
        h1 { margin-bottom: 20px; }
        .container { background-color: #1e1e1e; padding: 20px; border-radius: 10px; box-shadow: 0 4px 6px rgba(0,0,0,0.3); width: 300px; text-align: center; }
        .client-list { list-style: none; padding: 0; margin: 20px 0; max-height: 200px; overflow-y: auto; text-align: left; }
        .client-item { padding: 10px; border-bottom: 1px solid #333; display: flex; justify-content: space-between; }
        .client-item:last-child { border-bottom: none; }
        .client-id { font-size: 0.8em; color: #888; }
        button { background-color: #007bff; color: white; border: none; padding: 10px 20px; border-radius: 5px; cursor: pointer; font-size: 16px; transition: background-color 0.3s; }
        button.muted { background-color: #dc3545; }
        button:hover { opacity: 0.9; }
    </style>
</head>
<body>
    <h1>Audio Chat</h1>
    <div class="container">
        <h3>Connected Clients</h3>
        <ul id="client-list" class="client-list">
            <!-- Clients will be loaded here -->
        </ul>
        <button id="mute-btn" onclick="toggleMute()">Mute Microphone</button>
    </div>

    <script>
        async function fetchStatus() {
            try {
                const response = await fetch('/status');
                const data = await response.json();
                
                // Update Mute Button
                const btn = document.getElementById('mute-btn');
                if (data.muted) {
                    btn.textContent = "Unmute Microphone";
                    btn.classList.add('muted');
                } else {
                    btn.textContent = "Mute Microphone";
                    btn.classList.remove('muted');
                }

                // Update Client List
                const list = document.getElementById('client-list');
                list.innerHTML = '';
                data.clients.forEach(client => {
                    const li = document.createElement('li');
                    li.className = 'client-item';
                    li.innerHTML = `<span>${client.name}</span> <span class="client-id">(${client.id})</span>`;
                    list.appendChild(li);
                });
            } catch (e) {
                console.error("Error fetching status:", e);
            }
        }

        async function toggleMute() {
            try {
                await fetch('/mute', { method: 'POST' });
                fetchStatus();
            } catch (e) {
                console.error("Error toggling mute:", e);
            }
        }

        setInterval(fetchStatus, 1000);
        fetchStatus();
    </script>
</body>
</html>
    "#)
}

#[derive(Serialize)]
struct StatusResponse {
    muted: bool,
    clients: Vec<ClientInfo>,
}

async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let clients = state.clients.lock().unwrap().clone();
    let muted = *state.is_muted.lock().unwrap();
    Json(StatusResponse { muted, clients })
}

async fn toggle_mute(State(state): State<AppState>) {
    let mut muted = state.is_muted.lock().unwrap();
    *muted = !*muted;
}

pub async fn start_web_server(state: AppState, port: u16) {
    let app = Router::new()
        .route("/", get(index))
        .route("/status", get(get_status))
        .route("/mute", post(toggle_mute))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Web GUI running at http://127.0.0.1:{}", port);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
