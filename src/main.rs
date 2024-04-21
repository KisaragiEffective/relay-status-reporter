#![deny(clippy::all)]
#![warn(clippy::nursery)]

use std::sync::Arc;

use itertools::Itertools;
use reqwest::redirect::Policy;
use reqwest::tls::Version;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use url::Url;

#[derive(Deserialize)]
struct RelayInformationCollection(Vec<RelayInformationEntry>);

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RelayInformationEntry {
    url: Url,
}

#[derive(Serialize)]
struct MisskeyChartRequestPayload {
    host: String,
}

#[derive(Deserialize)]
struct IncomingNoteChartDelta {
    requests: PubSubCountDeltaCollection
}

#[derive(Deserialize)]
struct PubSubCountDeltaCollection {
    #[serde(rename = "received")]
    incoming: Vec<usize>
}

#[derive(Deserialize)]
struct LocalNoteChartData {
    local: LocalNoteChartInnerData,
}

#[derive(Deserialize)]
struct LocalNoteChartInnerData {
    inc: Vec<usize>,
    dec: Vec<usize>,
}

#[tokio::main]
async fn main() {
    let client = reqwest::ClientBuilder::new()
        .min_tls_version(Version::TLS_1_2)
        .user_agent("KisaragiMarine.RelayStatusReporter/1.0")
        .https_only(true)
        .redirect(Policy::none())
        .build()
        .expect("failed to initialize HTTP client");

    let connected_servers = client.get("https://relay.virtualkemomimi.net/api/servers")
        .header("Accept", "application/json")
        .send()
        .await
        .expect("http error")
        .json::<RelayInformationCollection>()
        .await
        .expect("valid JSON")
        .0;

    let mut join_handle_collection = JoinSet::new();

    let client = Arc::new(client);
    for s in connected_servers {
        const LIMIT: usize = 90;
        const PERIOD: &str = "hour";

        let t1 = s.url.clone();
        let host = t1.host_str().unwrap().to_string();
        let req_endpoint = std::env::var("REQUEST_ENDPOINT_BASE").expect("REQUEST_ENDPOINT");
        let req_endpoint = Url::parse(&req_endpoint).expect("valid URL");
        let locally = host == req_endpoint.host_str().expect("host");
        let req_endpoint = format!("{req_endpoint}?host={host}&limit={LIMIT}&span={PERIOD}");
        let req_endpoint = req_endpoint.clone();
        let client = client.clone();
        join_handle_collection.spawn(async move {
            let m = if locally {
                let r = client.get(format!("https://{host}/api/charts/notes?limit={LIMIT}&span={PERIOD}"))
                    .send()
                    .await
                    .expect("connection")
                    .json::<LocalNoteChartData>()
                    .await
                    .expect("valid JSON");

                let r = r.local;
                PubSubCountDeltaCollection {
                    incoming: r.inc.into_iter().zip(r.dec).map(|(inc, dec)| inc - dec).collect(),
                }
            } else {
                client.get(req_endpoint.clone())
                    .json(&MisskeyChartRequestPayload { host: host.clone() })
                    .send()
                    .await
                    .expect("connection")
                    .json::<IncomingNoteChartDelta>()
                    .await
                    .expect("valid")
                    .requests
            };

            (host, m)
        });
    }

    while !join_handle_collection.is_empty() {
        while let Some(conclusion) = join_handle_collection.join_next().await {
            let (host, records) = conclusion.expect("join");
            let records = records.incoming.into_iter().join(" ");
            eprintln!("handled: {host}, len: {}", join_handle_collection.len());
            println!("{host} {records}");
        }
    }
}
