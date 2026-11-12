pub mod address;
pub mod cache;
pub mod event;
pub mod log;
pub mod native;
pub mod tx;

pub use cache::EvmCtx;
pub use event::{EvmEvent, TransferDirection};

use crate::config::EvmConfig;
use alloy::{
    primitives::B256,
    providers::{Provider as _, ProviderBuilder, WsConnect},
    pubsub::{Subscription, SubscriptionStream},
    rpc::types::{Filter, Header, Log, Topic},
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
    #[snafu(display("日志订阅意外结束"))]
    SubscriptionEnded,
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
    /// 新区块头。每来一个区块就去拉完整区块，扫描原生币转账。
    /// 配置关掉原生币转账时为 [`None`]，不会订阅。
    blocks: Option<Subscription<Header>>,
}

pub async fn listen(
    cfg: EvmConfig,
    ct: CancellationToken,
    ev: EventTx,
) -> Result<(), EvmListenError> {
    let provider = connect_provider(&cfg.ws_url).await?;
    let filters = build_filters(&cfg)?;
    let subs = subscribe(&provider, &filters, cfg.native_transfer).await?;
    let ctx = EvmCtx::new(
        provider.clone(),
        cfg.address.iter().map(|x| (x.addr, x.name.clone())),
    );
    run_event_loop(subs, provider, ctx, ev, ct).await
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
    ensure!(!listen_addrs.is_empty(), EmptyAddressSnafu);
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
    native_transfer: bool,
) -> Result<Subscriptions, EvmListenError> {
    let incoming = provider
        .subscribe_logs(&filters.incoming)
        .await
        .context(SubLogsSnafu)?;
    let outgoing = provider
        .subscribe_logs(&filters.outgoing)
        .await
        .context(SubLogsSnafu)?;
    let blocks = if native_transfer {
        Some(provider.subscribe_blocks().await.context(SubBlocksSnafu)?)
    } else {
        None
    };
    Ok(Subscriptions {
        incoming,
        outgoing,
        blocks,
    })
}

async fn run_event_loop(
    subs: Subscriptions,
    provider: EvmProvider,
    ctx: EvmCtx,
    ev: EventTx,
    ct: CancellationToken,
) -> Result<(), EvmListenError> {
    let mut incoming_stream = subs.incoming.into_stream();
    let mut outgoing_stream = subs.outgoing.into_stream();
    let mut block_stream = subs.blocks.map(Subscription::into_stream);
    loop {
        tokio::select! {
            _ = ct.cancelled() => return Ok(()),

            // 订阅流结束说明 WebSocket 已经断开，交给上层重启
            incoming = incoming_stream.next() => {
                let Some(log) = incoming else {
                    return Err(EvmListenError::SubscriptionEnded);
                };
                // 收到代币 (to) / 被授权代币 (spender)
                log::spawn_log(log, TransferDirection::Recv, ctx.clone(), ev.clone(), ct.clone());
            }

            outgoing = outgoing_stream.next() => {
                let Some(log) = outgoing else {
                    return Err(EvmListenError::SubscriptionEnded);
                };
                // 花出代币 (from) / 授权代币给别人 (owner)
                log::spawn_log(log, TransferDirection::Send, ctx.clone(), ev.clone(), ct.clone());
            }

            header = next_block(&mut block_stream) => {
                let Some(header) = header else {
                    return Err(EvmListenError::SubscriptionEnded);
                };
                // 原生币转账没有日志，只能拿到完整区块后逐笔扫描
                native::spawn_block(
                    provider.clone(),
                    header.hash,
                    ctx.clone(),
                    ev.clone(),
                    ct.clone(),
                );
            }
        }
    }
}

/// 发送事件，接收端关闭时只记日志：监听不应该因为下游退出而中断。
pub(crate) async fn emit(ev: &EventTx, event: EvmEvent) {
    if ev.send(event).await.is_err() {
        tracing::warn!("发送 EVM 事件失败：接收端已关闭");
    }
}

/// 原生币转账关闭时这个 future 永远 pending，对应的 select 分支不会触发。
async fn next_block(stream: &mut Option<SubscriptionStream<Header>>) -> Option<Header> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}
