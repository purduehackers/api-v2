use axum::Router;

pub mod doorbell;
pub mod events;
pub mod phonebell;
pub mod printer;

pub trait EndpointModule {
    fn create_router() -> Router;
}
