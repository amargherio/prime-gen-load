use std::{thread::sleep, time::Duration, collections::BTreeMap};

use actix_web::{App, HttpResponse, HttpServer, web};
use k8s_openapi::api::{apps::v1::Deployment, core::v1::{Namespace, Pod, Service}};
use kube::{Api, Client, api::PostParams};
use rand::{Rng, distributions::Alphanumeric, thread_rng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing_actix_web::TracingLogger;

#[derive(Debug, Deserialize, Serialize)]
struct WorkloadConfig {
    count: usize,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // init tracing logging
    tracing_subscriber::fmt::init();

    HttpServer::new(move || {
    App::new()
        // logging
        .wrap(TracingLogger::default())
        .route("/init", web::put().to(init_workload))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    Ok(())

}

#[tracing::instrument]
async fn init_workload(workload: web::Query<WorkloadConfig>) -> HttpResponse {
    // create the initial Kube API client and create the target namespace for the workload (randomly-generated name used)
    let client = Client::try_default().await.unwrap();
    let target_ns = gen_target_ns().await.unwrap();
    tracing::info!("Generated namespace value {}", target_ns);
    let ns_api: Api<Namespace> = Api::all(client.clone());

    let mut ns: Namespace = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": target_ns
        }
    })).unwrap();

    match std::env::var("LINKERD_INJECT") {
        Ok(val) => {
            if val == "true" {
                tracing::info!("Linkerd injection is enabled - adding the correct annotation.");
                add_inject_annotation_to_ns(&mut ns);
            }
        },
        Err(_) => {
            tracing::warn!("'LINKERD_INJECT' variable not set - proceeding with workload init.");
        }
    }
    
    let ns_params = PostParams::default();
    match ns_api.create(&ns_params, &ns).await {
        Ok(n) => {
            let name = n.metadata.name.unwrap();
            tracing::info!("Created namespace {}", name);
        },
        Err(kube::Error::Api(ae)) => {
            // handle kubernetes specific errors here. this will most likely result in death
            // but needs more specific handling
            if ae.code == 401 {
                tracing::error!("Received an unauthorized response from the API server when creating namespace {}", target_ns);
            } else if ae.code == 429 {
                tracing::warn!("Received throttled response from API server - message: {}", ae.message);
            } else {
                tracing::warn!("Error occurred while attempting to interact with the API server. Status: {}, message: {}", ae.status, ae.message);
            }
            return HttpResponse::InternalServerError().finish();
        },
        Err(_e) => {
            unimplemented!()
        }
    }

    if workload.count <= 0 {
        tracing::warn!("Received request to spin up zero or a negative pod count - returning.");
        return HttpResponse::BadRequest().finish();
    }
    tracing::info!("Spinning up {} pods to calculate primes via sieve.", workload.count);
    tracing::debug!("Retrieving image URL information from env vars and stashing the container registry URL for later use.");
    let registry_url = std::env::var("CONTAINER_REGISTRY_BASE_PATH").unwrap();
    let instance_image_tag = std::env::var("INSTANCE_IMAGE").unwrap();
    let instance_image_url = format!("{}/{}", registry_url, instance_image_tag);

    deploy_instance_service(client.clone(), &target_ns, &instance_image_url).await;

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &target_ns);

    let dur = rand::thread_rng().gen_range(5000..=7000);
    tracing::debug!("Sleeping for {} milliseconds to give instance service a chance to start.", dur);
    sleep(Duration::from_millis(dur));

    // pull sieve image information from the environment for use in the deploy loop.
    let sieve_image_tag = std::env::var("SIEVE_IMAGE").unwrap();
    let sieve_image_url = format!("{}/{}", registry_url, sieve_image_tag);
    
    for n in 0..workload.count {
        let pod_def: Pod = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": format!("prime-sieve-instance-{}", n),
                "namespace": target_ns,
            },
            "spec": {
                "containers": [
                    {
                        "env": [
                            {
                                "name": "RUST_LOG",
                                "value": "info"
                            }
                        ],
                        "image": sieve_image_url,
                        "imagePullPolicy": "Always",
                        "name": "prime-generator",
                        "resources": {
                            "limits": {
                                "cpu": "500m",
                                "memory": "100Mi"
                            },
                            "requests": {
                                "cpu": "100m",
                                "memory": "50Mi"
                            }
                        }
                    }
                ],
                "restartPolicy": "Never"

            }
        })).unwrap();
        tracing::debug!("Generated sieve pod spec: {:#?}", pod_def);

        match pod_api.create(&PostParams::default(), &pod_def).await {
            Ok(_) => {
                tracing::debug!("Created new pod {} in namespace {}", format!("prime-sieve-instance-{}", n), target_ns);
            },
            Err(kube::Error::Api(ae)) => {
                // handle kubernetes specific errors here. this will most likely result in death
                // but needs more specific handling
                if ae.code == 401 {
                    tracing::error!("Received an unauthorized response from the API server when creating namespace {}", target_ns);
                } else if ae.code == 429 {
                    tracing::warn!("Received throttled response from API server - message: {}", ae.message);
                } else {
                    tracing::warn!("Error occurred while attempting to interact with the API server. Status: {}, message: {}", ae.status, ae.message);
                }
                return HttpResponse::InternalServerError().finish();
            },
            Err(e) => {
                tracing::error!("Unhandled error encountered: {:#?}", e);
            }
        }

        tracing::debug!("Brief sleep (150ms) before next pod creation");
        sleep(Duration::from_millis(150));
    }
    tracing::info!("Completed spin up of instance service and {} sieve pods.", workload.count);

    HttpResponse::Ok().finish()
}

#[tracing::instrument(skip(ns))]
fn add_inject_annotation_to_ns(ns: &mut Namespace) {
    let mut annts: BTreeMap<String, String> = BTreeMap::new();
    annts.insert(String::from("linkerd.io/inject"), String::from("enabled"));
    ns.metadata.annotations = Some(annts);
    tracing::debug!("Added 'linkerd.io/inject: enabled' to the namespace annotations.");
}

#[tracing::instrument(skip(client))]
async fn deploy_instance_service(client: Client, target_ns: &str, instance_image: &str) {
    // create instance service deployment and headless service in cluster
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), target_ns);
    let service_api: Api<Service> = Api::namespaced(client.clone(), target_ns);

    let dep: Deployment = serde_json::from_value(json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": "instance-service",
            "namespace": target_ns,
        },
        "spec": {
            "replicas": 1,
            "selector": {
                "matchLabels": {
                    "app": "instance-service"
                }
            },
            "template": {
                "metadata": {
                    "labels": {
                        "app": "instance-service"
                    }
                },
                "spec": {
                    "containers": [
                        {
                            "env": [
                                {
                                    "name": "RUST_LOG",
                                    "value": "info"
                                },
                                {
                                    "name": "REDIS_URL",
                                    "value": "localhost"
                                },
                                {
                                    "name": "REDIS_PORT",
                                    "value": "6379"
                                },
                                {
                                    "name": "REDIS_DB",
                                    "value": "primes"
                                }
                            ],
                            "name": "instance-service",
                            "image": instance_image,
                            "livenessProbe": {
                                "failureThreshold": 5,
                                "httpGet": {
                                    "path": "/health",
                                    "port": 8080,
                                    "scheme": "HTTP"
                                }
                            },
                            "ports": [
                                {
                                    "containerPort": 8080
                                }
                            ],
                            "readinessProbe": {
                                "failureThreshold": 5,
                                "httpGet": {
                                    "path": "/health",
                                    "port": 8080,
                                    "scheme": "HTTP"
                                },
                                "periodSeconds": 30,
                                "successThreshold": 1,
                                "timeoutSeconds": 5
                            },
                            "resources": {
                                "limits": {
                                    "cpu": "500m",
                                    "memory": "500Mi"
                                },
                                "requests": {
                                    "cpu": "100m",
                                    "memory": "250Mi"
                                }
                            }
                        },
                        {
                            "name": "redis",
                            "image": "redis:6.2.6",
                            "ports": [
                                {
                                    "containerPort": 6379,
                                    "name": "redis",
                                    "protocol": "TCP"
                                }
                            ],
                            "resources": {
                                "limits": {
                                    "cpu": "500m",
                                    "memory": "500Mi"
                                },
                                "requests": {
                                    "cpu": "100m",
                                    "memory": "250Mi"
                                }
                            }
                        }
                    ]
                }
            }
        }
    })).unwrap();

    let headless: Service = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": "instance-service-headless"
        },
        "spec": {
            "clusterIP": "None",
            "selector": {
                "app": "instance-service"
            },
            "ports": [
                {
                    "protocol": "TCP",
                    "port": 8080,
                    "targetPort": 8080
                }
            ]
        }
    })).unwrap();

    match deploy_api.create(&PostParams::default(), &dep).await {
        Ok(_) => {
            tracing::debug!("Created instance service deployment in target namesace '{}'", target_ns);
        },
        Err(kube::Error::Api(ae)) => {
            // handle kubernetes specific errors here. this will most likely result in death
            // but needs more specific handling
            if ae.code == 401 {
                tracing::error!("Received an unauthorized response from the API server when creating namespace {}", target_ns);
            } else if ae.code == 429 {
                tracing::warn!("Received throttled response from API server - message: {}", ae.message);
            } else {
                tracing::warn!("Error occurred while attempting to interact with the API server. Status: {}, message: {}", ae.status, ae.message);
            }
        },
        Err(e) => {
            tracing::error!("Unhandled error encountered: {:#?}", e);
        }
    }

    match service_api.create(&PostParams::default(), &headless).await {
        Ok(_) => {
            tracing::debug!("Created headless service in target namespace '{}'", target_ns)
        },
        Err(kube::Error::Api(ae)) => {
            // handle kubernetes specific errors here. this will most likely result in death
            // but needs more specific handling
            if ae.code == 401 {
                tracing::error!("Received an unauthorized response from the API server when creating namespace {}", target_ns);
            } else if ae.code == 429 {
                tracing::warn!("Received throttled response from API server - message: {}", ae.message);
            } else {
                tracing::warn!("Error occurred while attempting to interact with the API server. Status: {}, message: {}", ae.status, ae.message);
            }
        },
        Err(e) => {
            tracing::error!("Unhandled error encountered: {:#?}", e);
        }
    }
}

#[tracing::instrument]
async fn gen_target_ns() -> anyhow::Result<String> {
    let mut rng = thread_rng();

    let ns = (&mut rng)
    .sample_iter(Alphanumeric)
    .take(8)
    .map(char::from)
    .collect::<String>();
    
    let ns = ns.as_str().to_ascii_lowercase();

    Ok(ns)
}