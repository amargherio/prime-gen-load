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
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())

}

#[tracing::instrument]
async fn init_workload(workload: web::Query<WorkloadConfig>) -> HttpResponse {
    // create the initial Kube API client and create the target namespace for the workload (randomly-generated name used)
    let client = Client::try_default().await.unwrap();
    let target_ns = gen_target_ns().await.unwrap();
    tracing::debug!("Generated namespace value {}", target_ns);
    let ns_api: Api<Namespace> = Api::all(client.clone());

    let ns: Namespace = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": target_ns
        }
    })).unwrap();
    
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
    tracing::debug!("Spinning up {} pods to calculate primes via sieve.", workload.count);

    deploy_instance_service(client.clone(), &target_ns).await;

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &target_ns);
    
    for n in 0..=workload.count {
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
                        "image": "amartest.azurecr.io/apps/slb/prime-generator:0.1.0-1",
                        "name": "prime-generator"
                    }
                ],

            }
        })).unwrap();

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
            Err(_e) => {
                unimplemented!()
            }
        }
    }

    HttpResponse::Ok().finish()
}

#[tracing::instrument(skip(client))]
async fn deploy_instance_service(client: Client, target_ns: &str) {
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
                                }
                            ],
                            "name": "instance-service",
                            "image": "amartest.azurecr.io/apps/slb/instance-service:0.1.0-1",
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
                "matchLabels": {
                    "app": "instance-service"
                }
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
        Err(_) => {
            todo!()
        }
    }

    match service_api.create(&PostParams::default(), &headless).await {
        Ok(_) => {
            tracing::debug!("Created headless service in target namespace '{}'", target_ns)
        },
        Err(_) => {
            todo!()
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

    Ok(ns)
}