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
    pubsub::Subscription,
    rpc::types::{Filter, Log, Topic},
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

/// 日志订阅的过滤条件。incoming 命中 `to` / `spender`，outgoing 命中 `from` / `owner`。
struct Filters {
    incoming: Filter,
    outgoing: Filter,
}

struct Subscriptions {
    incoming: Subscription<Log>,
    outgoing: Subscription<Log>,
}

pub async fn listen(
    cfg: EvmConfig,
    ct: CancellationToken,
    ev: EventTx,
) -> Result<(), EvmListenError> {
    let provider = connect_provider(&cfg.ws_url).await?;
    let filters = build_filters(&cfg)?;
    let subs = subscribe(&provider, &filters).await?;
    let ctx = EvmCtx::new(
        provider,
        cfg.address.iter().map(|x| (x.addr, x.name.clone())),
    );
    run_event_loop(subs, ctx, ev, ct).await;
    Ok(())
}

async fn connect_provider(ws_url: &Url) -> Result<EvmProvider, EvmListenError> {
    let ws_conn = WsConnect::new(ws_url.as_str());
    ProviderBuilder::new()
        .connect_ws(ws_conn)
        .context(TransportSnafu {
            url: Arc::new(ws_url.clone()),
        })
        .await
}

fn build_filters(cfg: &EvmConfig) -> Result<Filters, EvmListenError> {
    let listen_addrs = Topic::from_iter(
        cfg.address
            .iter()
            .map(|x| B256::from_slice(x.addr.into_word().as_slice())),
    );
    ensure!(listen_addrs.len() > 0, EmptyAddressSnafu);
    let transfer_or_approval = Topic::from_iter([
        *log::TRANSFER_EVENT_SIGNATURE,
        *log::APPROVAL_EVENT_SIGNATURE,
    ]);
    let incoming = Filter::new()
        .event_signature(transfer_or_approval.clone())
        .topic2(listen_addrs.clone()); // to / spender
    let outgoing = Filter::new()
        .event_signature(transfer_or_approval)
        .topic1(listen_addrs); // from / owner
    Ok(Filters { incoming, outgoing })
}

async fn subscribe(
    provider: &EvmProvider,
    filters: &Filters,
) -> Result<Subscriptions, EvmListenError> {
    let incoming = provider
        .subscribe_logs(&filters.incoming)
        .await
        .context(SubLogsSnafu)?;
    let outgoing = provider
        .subscribe_logs(&filters.outgoing)
        .await
        .context(SubLogsSnafu)?;
    Ok(Subscriptions { incoming, outgoing })
}

async fn run_event_loop(subs: Subscriptions, ctx: EvmCtx, ev: EventTx, ct: CancellationToken) {
    let mut incoming_stream = subs.incoming.into_stream();
    let mut outgoing_stream = subs.outgoing.into_stream();
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
        }
    }
}
