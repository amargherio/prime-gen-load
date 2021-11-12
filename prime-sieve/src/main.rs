use std::time::Duration;

use rand::Rng;
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

#[derive(Serialize, Deserialize, Debug)]
struct RegisterPayload {
    id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ResultPayload {
    id: String,
    primes: Vec<usize>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // derive all primes up to a random number of primes
    // first we create our logger, then register with the instance service
    tracing_subscriber::fmt::init();

    let mut buf = uuid::Uuid::encode_buffer();
    let sieve_id = String::from(uuid::Uuid::new_v4().to_hyphenated().encode_lower(&mut buf));
    tracing::debug!("Sieve ID generated for this instance: {}", sieve_id);
    let register = RegisterPayload {
        id: sieve_id.clone(),
    };

    tracing::debug!("Creating HTTP client to interact with instance service");
    let client = reqwest::Client::new();
    //let resp: Response = client.post("http://instance-service-headless/register")
    let resp: Response = client.post("http://127.0.0.1:8080/register")
        .header("content-type", "application/json")
        .json(&register)
        .send()
        .await?;

    if resp.status() == StatusCode::CREATED {
        tracing::info!("Registered sieve worker with instance service, starting prime generation.");
    } else {
        tracing::warn!("Failed to register with instance sercice. Status code '{}' - continuing with work.", resp.status().as_u16());
    }

    // once registered, we start calculating primes
    let n = rand::thread_rng().gen_range(100000..=2500000);
    tracing::info!("Generating primes up to a limit of {}", n);
    let res = basic_sieve(n).await.collect::<Vec<_>>();
    tracing::info!("Generated prime number payload with {} entries. Building and sending results to instance service.", res.len());
    
    // after we hit our prime count, we send the results over to instance service and exit
    let result_payload = ResultPayload {
        id: sieve_id.clone(),
        primes: res
    };
    //let prime_res = client.put("http://instance-service-headless/result")
    let prime_res = client.put("http://127.0.0.1:8080/result")
        .header("content-type", "application/json")
        .json(&result_payload)
        .send()
        .await?;

    if prime_res.status() == StatusCode::OK {
        tracing::info!("Prime results accepted by instance service. Exiting.");
    } else {
        let status_num = prime_res.status().as_u16();
        let response_payload = prime_res.text().await?;
        if status_num >= 400 && status_num < 500 {
            tracing::error!("Client-side error response received: status code = {}, response = {}", status_num, response_payload);
        } else {
            tracing::warn!("Server-side error response received: status code = {}, response = {}", status_num, response_payload);
        }
    }

    Ok(())
}

async fn basic_sieve(limit: usize) -> Box<dyn Iterator<Item = usize>> {
    let mut is_prime = vec![true; (limit + 1).try_into().unwrap()];
    is_prime[0] = false;
    is_prime[1] = false;
    let limit_sqrt = (limit as f64).sqrt() as usize + 1;
    sleep(Duration::from_millis(5000)).await;

    for i in 2..limit_sqrt {
        if is_prime[i] {
            let mut multiple = i * i;
            while multiple <= limit {
                is_prime[multiple] = false;
                multiple += i;
            }
        }
    }

    sleep(Duration::from_millis(5000)).await;
    Box::new(is_prime.into_iter()
        .enumerate()
        .filter_map(|(p, is_prime)| if is_prime { Some(p) } else { None }))
}
