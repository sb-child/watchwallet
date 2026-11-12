use super::{
    EvmProvider,
    address::{AddressInfo, fetch_address_info},
    event::{AccountInfo, Erc20TokenInfo, TxInfo},
    tx::fetch_tx_info,
    tx::fetch_tx_info_at,
};
use alloy::primitives::{Address, B256};
use moka::future::Cache;
use std::{collections::HashMap, sync::Arc};

/// 地址信息缓存上限
// 地址是低频数据，基本只会增长到配置里的地址数加涉及的合约数。
const ADDR_CACHE_CAPACITY: u64 = 8_192;
/// 交易信息缓存上限
// 交易只会被同一笔交易的几条日志访问，没有必要长期保留。
const TX_CACHE_CAPACITY: u64 = 65_536;

// 约定：用 optionally_get_with 而不是 try_get_with，查询失败统一返回 None、不写缓存，等下次事件再试；
// 同一 key 的并发查询会被合并成一次 RPC。
/// 监听过程中共享的查询上下文，缓存按链各一份。
#[derive(Clone)]
pub struct EvmCtx {
    provider: EvmProvider,
    /// 配置里的地址名字，启动后不再变化
    names: Arc<HashMap<Address, Box<str>>>,
    addr_info: Cache<Address, AddressInfo>,
    tx_info: Cache<B256, TxInfo>,
}

impl EvmCtx {
    pub fn new(
        provider: EvmProvider,
        names: impl IntoIterator<Item = (Address, Box<str>)>,
    ) -> Self {
        Self {
            provider,
            names: Arc::new(names.into_iter().collect()),
            addr_info: Cache::builder().max_capacity(ADDR_CACHE_CAPACITY).build(),
            tx_info: Cache::builder().max_capacity(TX_CACHE_CAPACITY).build(),
        }
    }

    // 约定：地址信息只用于展示（是否合约），查询失败时退化为 EOA，避免丢失整条通知；
    // 失败结果不写缓存，下次事件会重新查询。
    /// 查询地址信息，并带上配置里写的名字。
    pub async fn account(&self, addr: Address) -> AccountInfo {
        let is_contract = matches!(self.lookup(addr).await, Some(info) if info.is_contract());
        AccountInfo {
            addr,
            name: self.names.get(&addr).cloned(),
            is_contract,
        }
    }

    /// 查询 ERC-20 代币信息。不是标准 ERC-20 时返回 [`None`]。
    pub async fn erc20(&self, contract: Address) -> Option<Erc20TokenInfo> {
        let AddressInfo::Erc20Contract(token) = self.lookup(contract).await? else {
            return None;
        };
        Some(Erc20TokenInfo {
            contract,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
        })
    }

    // 查询失败时事件照发，模板使用默认值。
    /// 查询交易信息。
    pub async fn tx(&self, txid: B256) -> Option<TxInfo> {
        self.tx_info
            .optionally_get_with(txid, fetch_tx_info(&self.provider, txid))
            .await
    }

    /// 查询交易信息，出块时间已知时省掉一次区块查询。
    pub async fn tx_at(&self, txid: B256, timestamp: u64) -> Option<TxInfo> {
        self.tx_info
            .optionally_get_with(txid, fetch_tx_info_at(&self.provider, txid, timestamp))
            .await
    }

    /// 该地址是否在配置的监听列表里。
    pub fn is_watched(&self, addr: Address) -> bool {
        self.names.contains_key(&addr)
    }

    async fn lookup(&self, addr: Address) -> Option<AddressInfo> {
        self.addr_info
            .optionally_get_with(addr, fetch_address_info(&self.provider, addr))
            .await
    }
}
