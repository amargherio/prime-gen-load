use actix_web::{
    error, middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer,
};
use futures::StreamExt;
use json::JsonValue;
use serde::{Deserialize, Serialize};
use tracing::{span, Level};
use tracing_actix_web::TracingLogger;

struct WorkloadConfig {
    count: usize,
}

#[actix_web::main]
async fn main() {
    // init tracing logging

    HttpServer::new(|| {
    App::new()
        // logging
        .wrap(TracingLogger)
        .service(
            web::resource("/init").route(web::put().to(init_workload)))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

#[tracing::instrument]
async fn init_workload(web::Query(count): web::Query<WorkloadConfig>) -> HttpResponse{
    // send the WorkloadConfig over a channel for async work and send the accepted response?

    HttpResponse::Accepted()
}