use std::sync::Arc;
use tracing::{error, info};

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::{types::Daemon, utils::get_my_id};

type SharedDaemons = Arc<Vec<Arc<RwLock<Daemon>>>>;

/// Represents the state of the API, including session information, participants, and active daemons.
///
/// # Fields
///
/// * `session_id` - A unique identifier for the current API session.
/// * `participants` - A list of participants involved in the session, each represented by a `ParticipantInfo` struct.
/// * `daemons` - A list of active daemons in the session, each represented by a `DaemonView` struct.
///
#[derive(Serialize)]
struct ApiState {
    session_id: u64,
    participants: Vec<ParticipantInfo>,
    daemons: Vec<DaemonView>,
}

/// Represents information about a participant in the system.
///
/// This struct is typically used to store and serialize details about a participant,
/// such as their unique identifier, the blockchain they are associated with, and their target value.
///
/// # Fields
///
/// * `id` - A unique identifier for the participant (u8).
/// * `blockchain` - The name of the blockchain associated with the participant (String).
/// * `target` - A numerical target value associated with the participant (u8).
///
#[derive(Serialize)]
struct ParticipantInfo {
    id: u8,
    blockchain: String,
    target: u8,
}

/// Represents the current state and details of a daemon instance.
///
/// This structure provides a snapshot of the specific details
/// regarding a daemon's state, history, and interactions within
/// its environment.
///
/// # Fields
///
/// * `my_id` - The unique identifier of the daemon.
/// * `state` - The current state of the daemon represented as a string.
/// * `state_history` - A history log of the states the daemon has undergone.
/// * `leader` - The identifier of the current leader in the daemon's context (if any).
/// * `confirmed_lock_txs` - A list of transactions that have been confirmed as locked.
/// * `broadcast_lock_txs` - A list of transactions that have been broadcasted as locked.
/// * `secrets_received` - A list of secrets or keys received by the daemon.
#[derive(Serialize)]
struct DaemonView {
    my_id: u8,
    state: String,
    state_history: Vec<String>,
    leader: Option<u8>,
    confirmed_lock_txs: Vec<u8>,
    broadcast_lock_txs: Vec<u8>,
    secrets_received: Vec<u8>,
}

/// Handles the API request to fetch the state of the system, including session details,
/// participant information, and daemon-specific views. It gathers and formats this data into
/// a structured JSON response.
///
/// # Parameters
///
/// - `State(daemons): State<SharedDaemons>`: A shared atomic reference containing the state
///   of all active daemons in the system.
///
/// # Returns
///
/// - `Json<ApiState>`: A serialized JSON object representing the current system state, which includes:
///   - `session_id`: The ID of the active session (if any).
///   - `participants`: A list of participant details for the active session, including:
///       - `id`: Participant's unique ID.
///       - `blockchain`: The blockchain associated with the participant.
///       - `target`: The participant's target counterpart in the session.
///   - `daemons`: A list of views for each daemon, with details including:
///       - `my_id`: The unique ID of the daemon.
///       - `state`: The current state of the daemon's session.
///       - `state_history`: A history of past states recorded for the daemon.
///       - `leader`: The ID of the leader participant for the daemon's session.
///       - `confirmed_lock_txs`: Sorted transaction IDs for confirmed locking transactions.
///       - `broadcast_lock_txs`: Sorted transaction IDs for broadcasted locking transactions.
///       - `secrets_received`: Sorted list of keys for adaptor secrets received.
///
/// # Errors
///
/// - This function does not directly return errors but assumes that the daemons
///   and session state are correctly initialized. If no active session exists,
///   default values are used.
async fn api_state(State(daemons): State<SharedDaemons>) -> Json<ApiState> {
    let mut daemon_views = Vec::new();
    let mut participant_info: Option<Vec<ParticipantInfo>> = None;
    let mut session_id = 0u64;

    for daemon_arc in daemons.iter() {
        let daemon = daemon_arc.read().await;
        if let Some((&sid, session)) = daemon.sessions.iter().next() {
            session_id = sid;

            if participant_info.is_none() {
                let mut infos: Vec<ParticipantInfo> = session
                    .participants
                    .values()
                    .map(|p| ParticipantInfo {
                        id: p.id,
                        blockchain: format!("{:?}", p.blockchain),
                        target: p.target_participant,
                    })
                    .collect();
                infos.sort_by_key(|p| p.id);
                participant_info = Some(infos);
            }

            let my_id = *get_my_id(&session.participants);
            let mut confirmed: Vec<u8> = session.confirmed_lock_txs.iter().copied().collect();
            confirmed.sort();
            let mut broadcast: Vec<u8> = session.lock_txs_broadcast.iter().copied().collect();
            broadcast.sort();
            let mut secrets: Vec<u8> = session.adaptor_secrets.keys().copied().collect();
            secrets.sort();

            daemon_views.push(DaemonView {
                my_id,
                state: format!("{:?}", session.state),
                state_history: session
                    .state_history
                    .iter()
                    .map(|s| format!("{:?}", s))
                    .collect(),
                leader: session.leader,
                confirmed_lock_txs: confirmed,
                broadcast_lock_txs: broadcast,
                secrets_received: secrets,
            });
        }
    }

    daemon_views.sort_by_key(|d| d.my_id);

    Json(ApiState {
        session_id,
        participants: participant_info.unwrap_or_default(),
        daemons: daemon_views,
    })
}

/// Middleware function to handle Cross-Origin Resource Sharing (CORS).
///
/// This function intercepts the incoming HTTP request, processes it using the next middleware
/// in the chain, and then modifies the response to include a CORS header allowing requests
/// from any origin (`*`).
///
/// # Parameters
///
/// * `req` - The incoming HTTP request to be processed.
/// * `next` - The next middleware or handler in the chain, responsible for further processing
///   the request and generating a response.
///
/// # Returns
///
/// A modified HTTP response with the `Access-Control-Allow-Origin` header set to `*`.
///
async fn cors(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    res
}

/// Starts an HTTP server to serve the dashboard API, providing access to daemon states.
///
/// This function initializes an HTTP server using the `axum` framework. It binds the server
/// to the specified port and provides a REST API endpoint (`/api/state`) to retrieve the state
/// of the daemons. The state is shared across requests using `Arc<RwLock<Daemon>>`.
///
/// # Parameters
/// - `daemons`: A vector of `Arc<RwLock<Daemon>>` representing the state of the daemons
///   to be shared across API requests.
/// - `port`: The port number on which the server will listen for incoming connections.
///
/// # Logs
/// - Logs an error if the specified port is already in use.
/// - Logs an informational message with the local URL where the dashboard is available.
///
/// # Panics
/// - The function will panic if the underlying `axum::serve()` call encounters an unrecoverable
///   error while serving the application.
///
/// # Notes
/// - Ensure that the `api_state` handler function and the `cors` middleware are implemented
///   correctly, and that the `Daemon` type satisfies the requirements for shared state operations
///   via `RwLock`.
pub async fn serve(daemons: Vec<Arc<RwLock<Daemon>>>, port: u16) {
    let state: SharedDaemons = Arc::new(daemons);
    let app = Router::new()
        .route("/api/state", get(api_state))
        .layer(middleware::from_fn(cors))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Dashboard: port {port} already in use, skipping ({e})");
            return;
        }
    };
    info!("Dashboard: http://localhost:{port}");
    axum::serve(listener, app).await.unwrap();
}
