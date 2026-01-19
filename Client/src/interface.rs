use axum::{
    extract::State,
    routing::{get, post},
    Json, Router, response::Html,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub is_muted: Arc<Mutex<bool>>,
}

// page HTML
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
            text-align : center;
            padding-bottom : 10px;
        }
        .subtitle {
            color: #888;
            font-size: 14px;
            margin-bottom: 24px;
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
        
        <button id="mute-btn" class="mute" onclick="toggleMute()">Mute Microphone</button>
    </div>

    <script>
        async function fetchStatus() {
            try {
                const response = await fetch('/status');
                const data = await response.json();
                
                // Update mute button
                const btn = document.getElementById('mute-btn');
                if (data.muted) {
                    btn.textContent = "Demute micro";
                    btn.className = 'unmute';
                } else {
                    btn.textContent = "Mute micro";
                    btn.className = 'mute';
                }
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
}

async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let muted = *state.is_muted.lock().unwrap();
    Json(StatusResponse { muted })
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
    println!("Interface web run sur http://127.0.0.1:{}", port);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}