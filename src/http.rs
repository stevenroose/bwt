use std::net;
use std::str::FromStr;
use std::sync::Arc;

use async_std::task;
use warp::http::StatusCode;
use warp::Filter;
use warp::{reply, Reply};

use bitcoin::{Address, Txid};

use crate::error::{Error, OptionExt};
use crate::types::ScriptHash;
use crate::util::address_to_scripthash;
use crate::Query;

#[tokio::main]
async fn run(addr: net::SocketAddr, query: Arc<Query>) {
    let query = warp::any().map(move || Arc::clone(&query));

    // Pre-processing
    // GET /address/:address/*
    // GET /scripthash/:scripthash/*
    let address_route = warp::path!("address" / String / ..)
        .map(|address: String| {
            let address = Address::from_str(&address)?;
            // TODO ensure!(address.network == config.network);
            let scripthash = address_to_scripthash(&address);
            Ok(scripthash)
        })
        .and_then(reject_error);
    let scripthash_route = warp::path!("scripthash" / String / ..)
        .map(|scripthash: String| Ok(ScriptHash::from_str(&scripthash)?))
        .and_then(reject_error);
    let spk_route = address_route.or(scripthash_route).unify();

    // GET /address/:address
    // GET /scripthash/:scripthash
    let spk_handler = spk_route
        .and(warp::path::end())
        .and(query.clone())
        .map(|scripthash, query: Arc<Query>| {
            let script_info = query.get_script_info(&scripthash).or_err("not found")?;
            Ok(reply::json(&script_info))
        })
        .map(handle_error);

    // GET /address/:address/history
    // GET /scripthash/:scripthash/history
    let spk_history_handler = spk_route
        .and(warp::path!("history"))
        .and(query.clone())
        .map(|scripthash, query: Arc<Query>| {
            let txs = query.get_history_info(&scripthash);
            Ok(reply::json(&txs))
        })
        .map(handle_error);

    // GET /address/:address/history/minimal
    // GET /scripthash/:scripthash/history/minimal
    let spk_minimal_history_handler = spk_route
        .and(warp::path!("history" / "minimal"))
        .and(query.clone())
        .map(|scripthash, query: Arc<Query>| {
            let txs = query.get_history(&scripthash);
            Ok(reply::json(&txs))
        })
        .map(handle_error);

    // Pre-processing
    // GET /tx/:txid/*
    let tx_route = warp::path("tx").and(
        warp::path::param()
            .map(|txid: String| Ok(Txid::from_str(&txid)?))
            .and_then(reject_error),
    );

    // GET /tx/:txid
    let tx_handler = tx_route
        .and(warp::path::end())
        .and(query.clone())
        .map(|txid: Txid, query: Arc<Query>| {
            let tx_info = query.get_tx_info(&txid).or_err("tx not found")?;
            Ok(reply::json(&tx_info))
        })
        .map(handle_error);

    // GET /tx/:txid/verbose
    let tx_verbose_handler = tx_route
        .and(warp::path("verbose"))
        .and(warp::path::end())
        .and(query.clone())
        .map(|txid: Txid, query: Arc<Query>| {
            let tx_json = query.get_tx_json(&txid)?;
            Ok(reply::json(&tx_json))
        })
        .map(handle_error);

    // GET /tx/:txid/hex
    let tx_hex_handler = tx_route
        .and(warp::path("hex"))
        .and(warp::path::end())
        .and(query.clone())
        .map(|txid: Txid, query: Arc<Query>| {
            let tx_raw = query.get_tx_raw(&txid)?;
            Ok(hex::encode(tx_raw))
        })
        .map(handle_error);

    let handlers = spk_handler
        .or(spk_history_handler)
        .or(spk_minimal_history_handler)
        .or(tx_handler)
        .or(tx_verbose_handler)
        .or(tx_hex_handler);
    let route = warp::get().and(handlers);

    info!("starting http server on {}", addr);

    warp::serve(route).run(addr).await
}

pub fn start_http(addr: net::SocketAddr, query: Arc<Query>) -> task::JoinHandle<()> {
    task::spawn(async move {
        run(addr, query);
    })
}

async fn reject_error<T>(result: Result<T, Error>) -> Result<T, warp::Rejection> {
    result.map_err(|err| {
        warn!("filter rejected: {:?}", err);
        warp::reject::custom(WarpError::Error(err))
    })
}

fn handle_error<T>(result: Result<T, Error>) -> impl Reply
where
    T: Reply + Send,
{
    match result {
        Ok(x) => x.into_response(),
        Err(e) => {
            warn!("request failed with: {:#?}", e);
            let status = StatusCode::INTERNAL_SERVER_ERROR;
            reply::with_status(e.to_string(), status).into_response()
        }
    }
}

#[derive(Debug)]
enum WarpError {
    Error(Error),
}

impl warp::reject::Reject for WarpError {}

/*
impl From<WarpError> for warp::reject::Rejection {
    fn from(e: WarpError) -> Self {
        warp::reject::custom(e)
    }
}

impl From<Error> for WarpError {
    fn from(e: Error) -> Self {
        WarpError::Error(e)
    }
}*/