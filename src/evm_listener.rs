use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use crate::config::EvmConfig;
use alloy::{
    primitives::{Address, B256, Keccak256, U256},
    providers::{Provider as _, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log, Topic},
    transports::{RpcError, TransportError, TransportErrorKind},
};
use snafu::{ResultExt, Snafu, ensure, futures::TryFutureExt};
use tokio_stream::StreamExt;
use url::Url;
use whirlwind::ShardMap;

/// ERC-20 / ERC-721 Transfer
static TRANSFER_EVENT_SIGNATURE: LazyLock<B256> = LazyLock::new(|| {
    let mut h = Keccak256::new();
    // ERC-20
    // T0 hash  T1 from T2 to   Data amount
    // Transfer(address,address,uint256)

    // ERC-721
    // T0 hash  T1 from T2 to   T3 tokenId  Data 0x  amount=1
    // Transfer(address,address,uint256)
    h.update("Transfer(address,address,uint256)");
    h.finalize()
});

/// ERC-20 Approval
static APPROVAL_EVENT_SIGNATURE: LazyLock<B256> = LazyLock::new(|| {
    let mut h = Keccak256::new();
    // ERC-20
    // T0 hash  T1 owner  T2 spender  Data amount
    // Approval(address,address,uint256)
    h.update("Approval(address,address,uint256)");
    h.finalize()
});

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

type AddrMap = ShardMap<Address, Box<str>>;

pub async fn listen(cfg: EvmConfig) -> Result<(), EvmListenError> {
    let ws_url = Arc::new(cfg.ws_url);

    let listen_addrs = Topic::from_iter(
        cfg.address
            .iter()
            .map(|x| B256::from_slice(x.addr.into_word().as_slice())),
    );
    ensure!(listen_addrs.len() > 0, EmptyAddressSnafu);
    let _addr_names: HashMap<Address, Box<str>> =
        HashMap::from_iter(cfg.address.iter().map(|x| (x.addr, x.name.clone())));
    let addr_names: AddrMap = ShardMap::new();
    for (k, v) in _addr_names {
        addr_names.insert(k, v).await;
    }
    let transfer_or_approval =
        Topic::from_iter([*TRANSFER_EVENT_SIGNATURE, *APPROVAL_EVENT_SIGNATURE]);

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

    let sub_blocks = provider.subscribe_blocks().await.context(SubBlocksSnafu)?;
    let sub_incoming = provider
        .subscribe_logs(&filter_incoming)
        .await
        .context(SubLogsSnafu)?;
    let sub_outgoing = provider
        .subscribe_logs(&filter_outgoing)
        .await
        .context(SubLogsSnafu)?;

    let mut incoming_stream = sub_incoming.into_stream();
    let mut outgoing_stream = sub_outgoing.into_stream();
    let mut block_stream = sub_blocks.into_stream();

    loop {
        tokio::select! {
            Some(log) = incoming_stream.next() => {
                on_incoming_log(log, addr_names.clone()).await;
            }

            Some(log) = outgoing_stream.next() => {
                on_outgoing_log(log, addr_names.clone()).await;
            }

            Some(header) = block_stream.next() => {
                // // 在 L2 上，为节省 RPC 消耗，只在需要时精细化查询该区块
                // if let Ok(Some(block)) = provider.get_block_by_number(header.number.into(), true).await {
                //     if let Some(txs) = block.transactions.as_full() {
                //         for tx in txs {
                //             if tx.to == Some(target_address) && tx.value > alloy::primitives::U256::ZERO {
                //                 tracing::info!("💰 [{}] 收到原生币转账: {} wei, Hash: {:?}", chain_name, tx.value, tx.hash);
                //             } else if tx.from == target_address {
                //                 if tx.to.is_none() {
                //                     tracing::info!("🏗️ [{}] 部署了新合约！Hash: {:?}", chain_name, tx.hash);
                //                 } else {
                //                     tracing::info!("📤 [{}] 发起原生币转账 / 合约交互, Hash: {:?}", chain_name, tx.hash);
                //                 }
                //             }
                //         }
                //     }
                // }
            }
        }
    }

    Ok(())
}

/// - 收到代币 (to)
/// - 被授权代币 (spender)
async fn on_incoming_log(log: Log, am: AddrMap) {
    let Some(t0) = log.topic0() else {
        return;
    };
    let topics = log.topics();
    let data = &log.data().data;
    if topics.len() != 3 {
        return;
    }
    if data.len() < 32 {
        return;
    }
    let coin_amount = U256::from_be_slice(&data[..32]);
    let coin_contract = log.address();
    if *t0 == *TRANSFER_EVENT_SIGNATURE {
        // ERC-20 Transfer
        let transfer_from = Address::from_word(topics[1]);
        let transfer_to = Address::from_word(topics[2]);
    } else if *t0 == *APPROVAL_EVENT_SIGNATURE {
        // ERC-20 Approval
        let approval_owner = Address::from_word(topics[1]);
        let approval_spender = Address::from_word(topics[2]);
    } else {
        return;
    }
}

/// - 花出代币 (from)
/// - 授权代币给别人 (owner)
async fn on_outgoing_log(log: Log, am: AddrMap) {}
