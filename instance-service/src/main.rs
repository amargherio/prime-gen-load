use std::{collections::HashMap, sync::{Arc, Mutex}};

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone)]
struct AppData {
    sieve_map: HashMap<String, Worker>
}


#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // init tracing logging
    tracing_subscriber::fmt::init();

    // init datastore for instance service
    let hmap: HashMap<String, Worker> = HashMap::new();
    //let mut store = Arc::new(Mutex::new(hmap));
    let store = web::Data::new(Mutex::new(AppData {
        sieve_map: hmap,
    }));

    HttpServer::new(move || {
    App::new()
        .app_data(store.clone())
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
async fn register_sieve(store: web::Data<Mutex<AppData>>, sieve: web::Json<Sieve>) -> HttpResponse {
    let worker = Worker { id: sieve.id.clone(), result: None };
    let id = sieve.id.clone();

    let mut hstore = store.try_lock().unwrap();
    let mut hmap = &mut hstore.sieve_map;
        
    tracing::debug!("Inserting ID '{}' and worker {:?} into hstore", id, worker);
    hmap.insert(id, worker);


    HttpResponse::Created().finish()
}

#[tracing::instrument(skip(payload))]
async fn save_result(store: web::Data<Mutex<AppData>>, payload: web::Json<SieveResult>) -> HttpResponse {
    let mut hstore = store.try_lock().unwrap();
    let mut hmap = &mut hstore.sieve_map;

    tracing::info!("Received result from worker {} with primes length {}", &payload.id, &payload.primes.len());

    match hmap.get(&payload.id) {
        Some(_) => {
            tracing::debug!("Updating results for worker record and saving to store");
            hmap.entry(payload.id.clone()).and_modify(|wo| { wo.result = Some(payload.primes.clone()) });
        },
        None => {
            tracing::warn!("Received results payload from worker {} that was not previously registered.", payload.id);
            let worker = Worker {
                id: payload.id.clone(),
                result: Some(payload.primes.clone())
            };
            hmap.insert(payload.id.clone(), worker);
        },
    }

    HttpResponse::Ok().finish()
}