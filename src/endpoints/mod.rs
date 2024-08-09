use axum::Router;

pub mod doorbell;
pub mod events;
pub mod phonebell;

pub trait EndpointModule {
    fn create_router() -> Router;
}
