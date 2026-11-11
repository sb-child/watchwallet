use alloy::primitives::Address;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// EVM 链配置
    pub evm: Box<[EvmConfig]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmConfig {
    /// 区块链名称
    pub chain_name: Box<str>,
    /// 原生币名称
    pub coin: Box<str>,
    /// 区块链 Websocket RPC 地址
    pub ws_url: Url,
    /// 监听地址
    pub address: Box<[EvmAddressConfig]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmAddressConfig {
    /// 地址名称
    pub name: Box<str>,
    /// 地址
    pub addr: Address,
}
