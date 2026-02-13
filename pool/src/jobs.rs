use serde::{Serialize, Deserialize};
use hyper::{Uri, Response};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use http_body_util::Empty;
use bytes::Bytes;
use hyper::body::Incoming;
use http_body_util::BodyExt;
use anyhow::Result;

#[derive(Serialize, Clone)]
pub struct Job {
    pub id: u64,
    pub data: String,
}

pub fn next(id: u64) -> Job { Job { id, data: "template".to_string() } }

#[derive(Deserialize)]
pub struct Template { pub version: u32, pub height: u64, pub difficulty: u64, pub prev_hash: String, pub target: String, pub reward_atomic: u64 }

pub async fn fetch_next(id: u64, core_url: &str) -> Result<Job> {
    let connector = HttpConnector::new();
    let client: Client<HttpConnector, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build(connector);
    let url = format!("{}/rpc/get_block_template", core_url);
    let uri: Uri = url.parse()?;
    let resp: Response<Incoming> = client.get(uri).await?;
    let bytes = resp.into_body().collect().await?.to_bytes();
    let tpl: Template = serde_json::from_slice(&bytes)?;
    let data = serde_json::json!({
        "version": tpl.version,
        "height": tpl.height,
        "difficulty": tpl.difficulty,
        "prev_hash": tpl.prev_hash,
        "target": tpl.target,
        "reward_atomic": tpl.reward_atomic
    }).to_string();
    Ok(Job { id, data })
}

pub async fn fetch_template(core_url: &str) -> Result<Template> {
    let connector = HttpConnector::new();
    let client: Client<HttpConnector, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build(connector);
    let url = format!("{}/rpc/get_block_template", core_url);
    let uri: Uri = url.parse()?;
    let resp: Response<Incoming> = client.get(uri).await?;
    let bytes = resp.into_body().collect().await?.to_bytes();
    let tpl: Template = serde_json::from_slice(&bytes)?;
    Ok(tpl)
}
