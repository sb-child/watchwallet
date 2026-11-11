use alloy::primitives::Address;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub evm: Box<[EvmConfig]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmConfig {
    pub chain_name: Box<str>,
    pub ws_url: Url,
    pub address: Box<[EvmAddressConfig]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmAddressConfig {
    pub name: Box<str>,
    pub addr: Address,
}
