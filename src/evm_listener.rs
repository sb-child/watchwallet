use crate::config::EvmConfig;
use alloy::{
    primitives::{Address, B256, Keccak256, U256},
    providers::{Provider as _, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log, Topic},
    sol,
    transports::{RpcError, TransportError, TransportErrorKind},
};
use crossfire::{flavor, mpmc};
use mytm::TaskManager;
use snafu::{ResultExt, Snafu, ensure, futures::TryFutureExt};
use std::sync::{Arc, LazyLock};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use url::Url;
use whirlwind::ShardMap;

sol! {
    #[sol(rpc)]
    interface IERC20Metadata {
        function name() external view returns (string);
        function symbol() external view returns (string);
        function decimals() external view returns (uint8);
    }
}

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
type AddrInfoMap = ShardMap<Address, AddressInfo>;
type FetcherRx = crossfire::MAsyncRx<mpmc::List<(Address, crossfire::oneshot::TxOneshot<()>)>>;
type FetcherTx = crossfire::MAsyncTx<mpmc::List<(Address, crossfire::oneshot::TxOneshot<()>)>>;
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

#[derive(Debug, Clone)]
enum AddressInfo {
    EOA,
    Erc20Contract(Erc20Contract),
    OtherContract,
}

#[derive(Debug, Clone)]
struct Erc20Contract {
    symbol: Option<Box<str>>,
    name: Option<Box<str>>,
    decimals: u8,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EOA => f.write_str("EOA"),
            Self::OtherContract => f.write_str("Contract"),
            Self::Erc20Contract(token) => {
                let symbol = token.symbol.as_deref().unwrap_or("Unknown");
                match token.name.as_deref() {
                    Some(name) if !name.is_empty() => {
                        write!(
                            f,
                            "Contract: ERC-20 {symbol} ({name}) decimals={}",
                            token.decimals
                        )
                    }
                    _ => write!(f, "Contract: ERC-20 {symbol} decimals={}", token.decimals),
                }
            }
        }
    }
}

async fn fetcher(p: EvmProvider, req_rx: FetcherRx, addr_info: AddrInfoMap, ct: CancellationToken) {
    loop {
        // 从 req_rx 拉取地址
        let (addr, tx) = tokio::select! {
            _ = ct.cancelled() => return,
            req = req_rx.recv() => match req {
                Ok(req) => req,
                // 所有发送端已关闭，不会再有请求
                Err(_) => return,
            },
        };
        // 如果 addr_info 有这个条目，立刻发回 ()
        if addr_info.contains_key(&addr).await {
            tx.send(());
            continue;
        }
        // 如果没有，通过 provider 拉取信息
        let info = tokio::select! {
            _ = ct.cancelled() => return,
            info = fetch_address_info(&p, addr) => match info {
                Ok(info) => info,
                // 如果 provider 出错，打印 tracing error，发回 ()，不要动 addr_info
                Err(err) => {
                    tracing::error!(address = %addr, "查询地址信息失败: {err}");
                    tx.send(());
                    continue;
                }
            },
        };
        // 写到 addr_info，然后再回 ()
        tracing::info!(address = %addr, "查询到地址信息: {info}");
        addr_info.insert(addr, info).await;
        tx.send(());
    }
}

/// 通过 provider 拉取地址信息。
///
/// - 节点正常响应但调用失败（例如合约没有对应方法而回滚）只说明该合约不是标准 ERC-20，归类为 [`AddressInfo::OtherContract`]
/// - 没有收到节点响应的传输错误则视为 provider 出错，返回错误且不写缓存。
async fn fetch_address_info(p: &EvmProvider, addr: Address) -> Result<AddressInfo, TransportError> {
    let code = p.get_code_at(addr).await?;
    if code.is_empty() {
        return Ok(AddressInfo::EOA);
    }

    let token = IERC20Metadata::new(addr, p.clone());
    let decimals = match token.decimals().call().await {
        Ok(decimals) => decimals,
        Err(alloy::contract::Error::TransportError(err)) if err.as_error_resp().is_none() => {
            return Err(err);
        }
        Err(_) => return Ok(AddressInfo::OtherContract),
    };
    let symbol = match token.symbol().call().await {
        Ok(symbol) => Some(symbol.into_boxed_str()),
        Err(alloy::contract::Error::TransportError(err)) if err.as_error_resp().is_none() => {
            return Err(err);
        }
        Err(_) => None,
    };
    let name = match token.name().call().await {
        Ok(name) => Some(name.into_boxed_str()),
        Err(alloy::contract::Error::TransportError(err)) if err.as_error_resp().is_none() => {
            return Err(err);
        }
        Err(_) => None,
    };

    Ok(AddressInfo::Erc20Contract(Erc20Contract {
        symbol,
        name,
        decimals,
    }))
}

pub async fn listen(
    cfg: EvmConfig,
    tm: TaskManager,
    ct: CancellationToken,
) -> Result<(), EvmListenError> {
    let ws_url = Arc::new(cfg.ws_url);
    let listen_addrs = Topic::from_iter(
        cfg.address
            .iter()
            .map(|x| B256::from_slice(x.addr.into_word().as_slice())),
    );
    ensure!(listen_addrs.len() > 0, EmptyAddressSnafu);
    let addr_names: AddrMap =
        ShardMap::from_iter(cfg.address.iter().map(|x| (x.addr, x.name.clone())));
    let transfer_or_approval =
        Topic::from_iter([*TRANSFER_EVENT_SIGNATURE, *APPROVAL_EVENT_SIGNATURE]);
    let addr_info: AddrInfoMap = ShardMap::new();
    let (_fetcher_req_tx, fetcher_req_rx) = mpmc::unbounded_async();
    let fetcher_req_tx = _fetcher_req_tx.into_async();
    // fliters
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
    // fetcher task
    let frx = fetcher_req_rx.clone();
    let p = provider.clone();
    let ai = addr_info.clone();
    let fetcher_task: mytm::TaskHandle<(), (), ()> = tm.spawn_typed(|_tm, pu, ct| async move {
        pu.update(());
        fetcher(p, frx, ai, ct).await;
        Ok(())
    });
    fetcher_task.cancel_at(ct);
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

    let mut incoming_stream = sub_incoming.into_stream();
    let mut outgoing_stream = sub_outgoing.into_stream();
    let mut block_stream = sub_blocks.into_stream();

    loop {
        tokio::select! {
            Some(log) = incoming_stream.next() => {
                on_incoming_log(log, fetcher_req_tx.clone(), addr_names.clone()).await;
            }

            Some(log) = outgoing_stream.next() => {
                on_outgoing_log(log, fetcher_req_tx.clone(), addr_names.clone()).await;
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
async fn on_incoming_log(log: Log, ftx: FetcherTx, am: AddrMap) {
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
async fn on_outgoing_log(log: Log, ftx: FetcherTx, am: AddrMap) {}
