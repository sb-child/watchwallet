pub mod address;
pub mod cache;
pub mod event;
pub mod log;
pub mod tx;

pub use cache::EvmCtx;
pub use event::{EvmEvent, TransferDirection};

use crate::config::EvmConfig;
use alloy::{
    primitives::B256,
    providers::{Provider as _, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Topic},
    transports::{RpcError, TransportError, TransportErrorKind},
};
use crossfire::mpmc;
use snafu::{ResultExt, Snafu, ensure, futures::TryFutureExt};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use url::Url;

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

type EvmProvider = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::Identity,
        alloy::providers::fillers::JoinFill<
            alloy::providers::fillers::GasFiller,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::BlobGasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::NonceFiller,
                    alloy::providers::fillers::ChainIdFiller,
                >,
            >,
        >,
    >,
    alloy::providers::RootProvider,
>;

/// 每条链一个事件通道，事件本身不带链信息。
pub type EventRx = crossfire::MAsyncRx<mpmc::List<EvmEvent>>;
pub type EventTx = crossfire::MAsyncTx<mpmc::List<EvmEvent>>;

pub async fn listen(
    cfg: EvmConfig,
    ct: CancellationToken,
    ev: EventTx,
) -> Result<(), EvmListenError> {
    let ws_url = Arc::new(cfg.ws_url);
    let listen_addrs = Topic::from_iter(
        cfg.address
            .iter()
            .map(|x| B256::from_slice(x.addr.into_word().as_slice())),
    );
    ensure!(listen_addrs.len() > 0, EmptyAddressSnafu);
    // fliters
    let transfer_or_approval = Topic::from_iter([
        *log::TRANSFER_EVENT_SIGNATURE,
        *log::APPROVAL_EVENT_SIGNATURE,
    ]);
    let filter_incoming = Filter::new()
        .event_signature(transfer_or_approval.clone())
        .topic2(listen_addrs.clone()); // to / spender
    let filter_outgoing = Filter::new()
        .event_signature(transfer_or_approval)
        .topic1(listen_addrs); // from / owner
    // connect to websocket
    let ws_conn = WsConnect::new(ws_url.as_str());
    let provider = ProviderBuilder::new()
        .connect_ws(ws_conn)
        .context(TransportSnafu { url: ws_url })
        .await?;
    let ctx = EvmCtx::new(
        provider.clone(),
        cfg.address.iter().map(|x| (x.addr, x.name.clone())),
    );
    // subscriptions
    let sub_blocks = provider.subscribe_blocks().await.context(SubBlocksSnafu)?;
    let sub_incoming = provider
        .subscribe_logs(&filter_incoming)
        .await
        .context(SubLogsSnafu)?;
    let sub_outgoing = provider
        .subscribe_logs(&filter_outgoing)
        .await
        .context(SubLogsSnafu)?;
    // streams
    let mut incoming_stream = sub_incoming.into_stream();
    let mut outgoing_stream = sub_outgoing.into_stream();
    let mut block_stream = sub_blocks.into_stream();
    // event loop
    loop {
        tokio::select! {
            _ = ct.cancelled() => break,

            Some(log) = incoming_stream.next() => {
                // 收到代币 (to) / 被授权代币 (spender)
                log::spawn_log(log, TransferDirection::Recv, ctx.clone(), ev.clone(), ct.clone());
            }

            Some(log) = outgoing_stream.next() => {
                // 花出代币 (from) / 授权代币给别人 (owner)
                log::spawn_log(log, TransferDirection::Send, ctx.clone(), ev.clone(), ct.clone());
            }

            Some(_header) = block_stream.next() => {
                // 原生币转账需要在这里按区块扫描交易，对应 template/evm-native-*.typ
            }
        }
    }
    Ok(())
}
