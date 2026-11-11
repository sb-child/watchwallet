use std::{collections::HashMap, sync::Arc};

use crate::config::EvmConfig;
use alloy::{
    primitives::{Address, B256, Keccak256},
    providers::{Provider as _, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Topic},
    transports::{RpcError, TransportError, TransportErrorKind},
};
use lazy_static::lazy_static;
use snafu::{ResultExt, Snafu, ensure, futures::TryFutureExt};
use url::Url;

lazy_static! {
    /// ERC-20 / ERC-721 Transfer
    static ref TRANSFER_EVENT_SIGNATURE: B256 = {
        let mut h = Keccak256::new();
        //        T0 hash  T1 from T2 to   Data amount
        h.update("Transfer(address,address,uint256)");
        h.finalize()
    };
}

#[derive(Debug, Snafu)]
pub enum EvmListenError {
    #[snafu(display("Transport Error for {}", url))]
    Transport {
        source: TransportError,
        url: Arc<Url>,
    },
    SubBlocks {
        source: RpcError<TransportErrorKind>,
    },
    SubLogs {
        source: RpcError<TransportErrorKind>,
    },
    #[snafu(display("No Address Provided."))]
    EmptyAddress,
}

pub async fn listen(cfg: EvmConfig) -> Result<(), EvmListenError> {
    let ws_url = Arc::new(cfg.ws_url);
    let listen_addrs = Topic::from_iter(
        cfg.address
            .iter()
            .map(|x| B256::from_slice(x.addr.into_word().as_slice())),
    );
    ensure!(listen_addrs.len() > 0, EmptyAddressSnafu);
    let addr_names: HashMap<Address, Box<str>> =
        HashMap::from_iter(cfg.address.iter().map(|x| (x.addr, x.name.clone())));
    let ws_conn = WsConnect::new(ws_url.as_str());
    let provider = ProviderBuilder::new()
        .connect_ws(ws_conn)
        .context(TransportSnafu { url: ws_url })
        .await?;

    let filter_incoming = Filter::new()
        .event_signature(*TRANSFER_EVENT_SIGNATURE)
        .topic2(listen_addrs.clone()); // to
    let filter_outgoing = Filter::new()
        .event_signature(*TRANSFER_EVENT_SIGNATURE)
        .topic1(listen_addrs); // from

    let sub_blocks = provider.subscribe_blocks().await.context(SubBlocksSnafu)?;
    let sub_incoming = provider
        .subscribe_logs(&filter_incoming)
        .await
        .context(SubLogsSnafu)?;
    let sub_outgoing = provider
        .subscribe_logs(&filter_outgoing)
        .await
        .context(SubLogsSnafu)?;

    Ok(())
}
