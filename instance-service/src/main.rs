use std::{collections::HashMap, sync::Arc};

use actix_web::{App, FromRequest, HttpRequest, HttpResponse, HttpServer, web};
use futures::StreamExt;
use json::JsonValue;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{span, Level};
use tracing_actix_web::TracingLogger;

#[derive(Debug, Deserialize, Serialize)]
struct Sieve {
    id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SieveResult {
    id: String,
    primes: Vec<i32>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Hash, Clone)]
struct Worker {
    id: String,
    result: Option<Vec<i32>>,
}


#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // init tracing logging

    // init datastore for instance service
    let mut store = HashMap::<String, Worker>::new();

    HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(store.clone()))
        // logging
        .wrap(TracingLogger::default())
        .route("/register", web::post().to(register_sieve))
        .route("/result", web::put().to(save_result))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}

#[tracing::instrument(skip(sieve))]
async fn register_sieve(store: web::Data<HashMap<String, Worker>>, sieve: web::Form<Sieve>) -> HttpResponse {
    let worker = Worker { id: sieve.id.clone(), result: None };

    let mut mut_store = store.into_inner();

    let hstore = Arc::get_mut(&mut mut_store).unwrap();
    hstore.insert(sieve.id.clone(), worker);


    HttpResponse::Created().finish()
}

#[tracing::instrument]
async fn save_result(store: web::Data<HashMap<String, Worker>>, payload: web::Form<SieveResult>) -> HttpResponse {
    let mut store = store.into_inner();
    let hstore = Arc::get_mut(&mut store).unwrap();

    match hstore.get(&payload.id) {
        Some(w) => {
            let mut worker_obj = w.clone();
            worker_obj.result = Some(payload.primes.clone());
            hstore.insert(payload.id.clone(), worker_obj);
        },
        None => {
            tracing::warn!("Received results payload from worker {} that was not previously registered.", payload.id);
            let worker = Worker {
                id: payload.id.clone(),
                result: Some(payload.primes.clone())
            };
            hstore.insert(payload.id.clone(), worker);
        },
    }

    HttpResponse::Ok().finish()
}