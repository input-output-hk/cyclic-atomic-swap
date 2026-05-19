use tracing::{error, info};
use crate::{
    networking::broadcast,
    types::{Envelope, SwapSession, WireMessage},
    utils::{get_my_id, get_other_addresses},
};

/// Asynchronously broadcasts the adaptor point of the current participant to the other participants in the swap session.
///
/// # Parameters
///
/// * `session` - A reference to the `SwapSession` containing information about the swap session,
///   including participants, adaptor points, and connection pool.
///
/// # Logging
///
/// - Logs an informational message when broadcasting begins, indicating the participant's
///   ID and the number of peers to which the message is sent.
/// - Logs an error message in case the broadcast fails.
///
/// # Errors
///
/// Errors that occur during broadcasting are logged using the `error!` macro but are not
/// propagated to the caller.
///
pub async fn broadcast_adaptor_point(session: &SwapSession) {
    let my_id = session.participants.values().find(|p| p.is_me).unwrap().id;
    let my_point = session.adaptor_points.get(&my_id).unwrap().clone();
    let other_addresses = get_other_addresses(&session.participants);
    info!("broadcasting adaptor point from participant {my_id} to {} peers", other_addresses.len());
    let envelope = Envelope::new(
        session.id,
        *get_my_id(&session.participants),
        WireMessage::AdaptorPoint(my_point),
    );

    if let Err(e) = broadcast(&other_addresses, &envelope, &session.connection_pool).await {
        error!("broadcast failed: {e}");
    }
}
