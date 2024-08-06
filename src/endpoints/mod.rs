use axum::Router;

pub mod doorbell;
pub mod events;

pub trait EndpointModule {
    fn create_router() -> Router;
}
