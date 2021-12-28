use std::{collections::HashMap, sync::Mutex, thread::sleep, time::Duration};

use actix_web::{App, HttpResponse, HttpServer, web};
use rand::Rng;
use redis::Commands;
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
    results: Option<PrimeResult>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Hash, Clone)]
struct PrimeResult {
    quantity: usize,
    max_prime: i32,
}

#[derive(Debug, Clone)]
struct AppData {
    sieve_map: HashMap<String, Worker>,
    redis: redis::Client,
}


#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // init tracing logging
    tracing_subscriber::fmt::init();

    // init datastore for instance service
    let hmap: HashMap<String, Worker> = HashMap::new();
    tracing::debug!("Completed hashmap creation for storing results.");

    // build redis client and wrap it for use as data
    let redis_url = std::env::var("REDIS_URL")?;
    let redis_port = std::env::var("REDIS_PORT")?;
    //let redis_db = std::env::var("REDIS_DB")?;
    //let formatted_conn_string = format!("redis://{}:{}/{}", redis_url, redis_port, redis_db);
    let formatted_conn_string = format!("redis://{}:{}/", redis_url, redis_port);
    tracing::debug!("Built formatted connection string for Redis - {}", formatted_conn_string);

    let client = redis::Client::open(formatted_conn_string.as_str())?;

    let store = web::Data::new(Mutex::new(AppData {
        sieve_map: hmap,
        redis: client,
    }));
    tracing::info!("Build AppData object with HashMap for local storage and Redis client for remote data");


    HttpServer::new(move || {
    App::new()
        .app_data(store.clone())
        // logging
        .wrap(TracingLogger::default())
        .route("/register", web::post().to(register_sieve))
        .route("/result", web::put().to(save_result))
        .route("/health", web::get().to(health_check))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    Ok(())
}

#[tracing::instrument(skip(store))]
async fn register_sieve(store: web::Data<Mutex<AppData>>, sieve: web::Json<Sieve>) -> HttpResponse {
    let worker = Worker { id: sieve.id.clone(), results: None };
    let id = sieve.id.clone();

    let mut hstore = store.try_lock().unwrap();
    let hmap = &mut hstore.sieve_map;
        
    tracing::info!("Inserting ID '{}' and worker {:?} into hstore", id, worker);
    hmap.insert(id, worker);
    let dur = rand::thread_rng().gen_range(400..=1000);
    sleep(Duration::from_millis(dur));

    HttpResponse::Created().finish()
}

#[tracing::instrument(skip(payload, store))]
async fn save_result(store: web::Data<Mutex<AppData>>, payload: web::Json<SieveResult>) -> HttpResponse {
    let mut hstore = store.try_lock().unwrap();
    let hmap = &mut hstore.sieve_map;

    tracing::info!("Received result from worker {} with primes length {}", &payload.id, &payload.primes.len());
    let prime_res = PrimeResult {
        max_prime: payload.primes.get(payload.primes.len() - 1).unwrap().clone(),
        quantity: payload.primes.len()
    };

    match hmap.get(&payload.id) {
        Some(_) => {
            tracing::debug!("Updating results for worker record and saving to store");
            hmap.entry(payload.id.clone()).and_modify(|wo| { wo.results = Some(prime_res.clone()) });
            
            // commit the max value to redis as well
            let redis = &hstore.redis;
            let mut con = redis.get_connection().unwrap();
            let _:() = con.set(payload.id.clone(), prime_res.max_prime.clone()).unwrap();
        },
        None => {
            tracing::warn!("Received results payload from worker {} that was not previously registered.", payload.id);
            let worker = Worker {
                id: payload.id.clone(),
                results: Some(prime_res.clone())
            };
            hmap.insert(payload.id.clone(), worker);
            
            // commit the max value to redis as well
            let redis = &hstore.redis;
            let mut con = redis.get_connection().unwrap();
            let _:() = con.set(payload.id.clone(), prime_res.max_prime.clone()).unwrap();
        },
    }

    HttpResponse::Ok().finish()
}

#[tracing::instrument]
async fn health_check() -> HttpResponse {
    tracing::info!("Responding to health check request with OK response.");
    HttpResponse::Ok().finish()
}