use axum::{
    extract::State,
    routing::{get, post},
    Json, Router, response::Html,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use tower_http::services::ServeDir;

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

// Serve the HTML page with clean, simple UI
async fn index() -> Html<&'static str> {
    Html(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Rust Audio Chat</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { 
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #1a1a1a;
            color: #e0e0e0;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            padding: 20px;
        }
        .container { 
            background: #242424;
            border-radius: 12px;
            padding: 30px;
            width: 100%;
            max-width: 500px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.3);
        }
        h1 { 
            font-size: 24px;
            margin-bottom: 8px;
            font-weight: 600;
        }
        .subtitle {
            color: #888;
            font-size: 14px;
            margin-bottom: 24px;
        }
        .status-bar {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 12px 16px;
            background: #2a2a2a;
            border-radius: 8px;
            margin-bottom: 20px;
            font-size: 14px;
        }
        .status-indicator {
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background: #4ade80;
        }
        .dot.disconnected { background: #ef4444; }
        .section-title {
            font-size: 16px;
            font-weight: 500;
            margin-bottom: 12px;
            color: #ccc;
        }
        .client-list {
            background: #2a2a2a;
            border-radius: 8px;
            margin-bottom: 20px;
            max-height: 300px;
            overflow-y: auto;
        }
        .client-item {
            padding: 14px 16px;
            border-bottom: 1px solid #333;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .client-item:last-child { border-bottom: none; }
        .client-info { display: flex; align-items: center; gap: 12px; }
        .client-avatar {
            width: 36px;
            height: 36px;
            border-radius: 50%;
            background: #444;
            display: flex;
            align-items: center;
            justify-content: center;
            font-weight: 600;
            font-size: 14px;
        }
        .client-name { font-weight: 500; }
        .client-id { 
            font-size: 12px;
            color: #666;
            margin-top: 2px;
        }
        .empty-state {
            text-align: center;
            padding: 40px 20px;
            color: #666;
        }
        .logo {
            display: block;
            margin: 0 auto -10px auto;
            width: 150px;         
            height: auto;
        }
        button {
            width: 100%;
            padding: 14px;
            border: none;
            border-radius: 8px;
            font-size: 15px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.2s;
        }
        h1 {
            text-align : center;
            padding-bottom : 10px;
        }
        button.mute {
            background: #3b82f6;
            color: white;
        }
        button.mute:hover { background: #2563eb; }
        button.unmute {
            background: #ef4444;
            color: white;
        }
        
        button.unmute:hover { background: #dc2626; }
    </style>
</head>
<body>
    <div class="container">
        <img src="/static/logo.png" alt="RustCord Logo" class="logo">
        <h1>RustCord</h1>
        <div class="status-bar">
            <div class="status-indicator">
                <div class="dot" id="status-dot"></div>
                <span id="status-text">Connecté</span>
            </div>
        </div>

        <div class="section-title">Personnes connectées (<span id="client-count">0</span>)</div>
        <div class="client-list" id="client-list">
            <div class="empty-state">Aucune personnes connectées</div>
        </div>

        <button id="mute-btn" class="mute" onclick="toggleMute()">Mute Microphone</button>
    </div>

    <script>
        async function fetchStatus() {
            try {
                const response = await fetch('/status');
                const data = await response.json();
                
                // Update status
                const statusDot = document.getElementById('status-dot');
                const statusText = document.getElementById('status-text');
                statusDot.classList.remove('disconnected');
                statusText.textContent = 'Connecté';
                
                // Update mute button
                const btn = document.getElementById('mute-btn');
                if (data.muted) {
                    btn.textContent = "Demute micro";
                    btn.className = 'unmute';
                } else {
                    btn.textContent = "Mute micro";
                    btn.className = 'mute';
                }

                // Update client list
                const list = document.getElementById('client-list');
                const count = document.getElementById('client-count');
                count.textContent = data.clients.length;
                
                if (data.clients.length === 0) {
                    list.innerHTML = '<div class="empty-state">No clients connected</div>';
                } else {
                    list.innerHTML = data.clients.map(client => `
                        <div class="client-item">
                            <div class="client-info">
                                <div class="client-avatar">${client.name.charAt(0).toUpperCase()}</div>
                                <div>
                                    <div class="client-name">${client.name}</div>
                                    <div class="client-id">ID: ${client.id}</div>
                                </div>
                            </div>
                        </div>
                    `).join('');
                }
            } catch (e) {
                console.error("Error fetching status:", e);
                document.getElementById('status-dot').classList.add('disconnected');
                document.getElementById('status-text').textContent = 'Déconnecté';
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
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Web GUI running at http://127.0.0.1:{}", port);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}